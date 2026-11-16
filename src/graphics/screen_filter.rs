use std::sync::Arc;

use super::{
    DrawCommand, DrawList, FrameTexture, ProjectedVertex, Rect, RenderTarget, Surface,
    TextureFilter, TextureSource,
};
use crate::{Result, read};

#[derive(Clone, Copy)]
struct Quad {
    vertices: [ProjectedVertex; 4],
    uv: [[f32; 2]; 4],
}

impl Quad {
    fn rectangle(rect: [f32; 4], extent: [f32; 2]) -> Self {
        let [x, y, width, height] = rect;
        Self {
            vertices: [
                [x, y],
                [x + width, y],
                [x, y + height],
                [x + width, y + height],
            ]
            .map(|p| ProjectedVertex {
                position: [p[0] - 0.5, p[1] - 0.5, 0.5],
                rhw: 1.,
                color: u32::MAX,
            }),
            uv: [
                [0., 0.],
                [(x + width) / extent[0], 0.],
                [0., (y + height) / extent[1]],
                [(x + width) / extent[0], (y + height) / extent[1]],
            ],
        }
    }

    fn commands(
        self,
        image: Arc<Surface>,
        size: [u32; 2],
        filter: Option<TextureFilter>,
    ) -> [DrawCommand; 2] {
        [[0, 1, 2], [2, 1, 3]].map(|indices| DrawCommand {
            vertices: indices.map(|i| self.vertices[i]),
            texture: Some(FrameTexture {
                source: TextureSource::Image(image.clone()),
                uv: indices.map(|i| self.uv[i]),
                address: Default::default(),
                filter,
            }),
            clip: Some(Rect::from_xywh(0, 0, size[0] as i32, size[1] as i32)),
            blend_mode: 0,
        })
    }
}

struct Blur {
    matrix: [f32; 16],
    offset: [f32; 3],
    color: u32,
    quad: Quad,
    first_frame: bool,
    index: usize,
}

struct Mosaic {
    scale: [f32; 2],
    divisors: [f32; 2],
    color: u32,
    quad: Option<Quad>,
}

enum Filter {
    Blur(Blur),
    Mosaic(Mosaic),
}

pub(super) struct ScreenFilter {
    pub active: u32,
    pub range: [f32; 2],
    size: [u32; 2],
    texture_size: [u32; 2],
    effect: Filter,
}

impl ScreenFilter {
    pub fn new(size: [u32; 2], scale: [f32; 2], mosaic: bool) -> Self {
        // 固定采用原设备的二次幂纹理分支，使后端能力不改变取样几何。
        let texture_size = size.map(u32::next_power_of_two);
        let effect = if mosaic {
            Filter::Mosaic(Mosaic {
                scale,
                divisors: [1.; 2],
                color: 0,
                quad: None,
            })
        } else {
            Filter::Blur(Blur {
                matrix: *super::Transform::default().matrix(),
                offset: [0.; 3],
                color: 0,
                quad: Quad::rectangle(
                    [0., 0., size[0] as f32, size[1] as f32],
                    texture_size.map(|v| v as f32),
                ),
                first_frame: true,
                index: 0,
            })
        };
        Self {
            active: 0,
            range: [0., 1_000_000.],
            size,
            texture_size,
            effect,
        }
    }

    pub fn configure(&mut self, bytes: &[u8]) -> Result<()> {
        let count = if matches!(self.effect, Filter::Mosaic(_)) {
            36
        } else {
            88
        };
        let bytes = read::range(bytes, 0, count)?;
        let word = |i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let float = |i| f32::from_bits(word(i));
        match &mut self.effect {
            Filter::Blur(blur) => {
                blur.matrix = std::array::from_fn(float);
                blur.offset = std::array::from_fn(|i| float(i + 16));
                blur.color = word(19);
                self.range = [float(20), float(21)];
            }
            Filter::Mosaic(mosaic) => {
                mosaic.divisors = [float(4), float(5)];
                mosaic.color = word(8);
                self.range = [float(6), float(7)];
                if mosaic.quad.is_none() {
                    let rect = std::array::from_fn(|i| float(i) * mosaic.scale[i % 2]);
                    mosaic.quad = Some(Quad::rectangle(rect, self.texture_size.map(|v| v as f32)));
                }
            }
        }
        Ok(())
    }

    pub fn set_active(&mut self, active: u32) {
        self.active = active;
        if let Filter::Blur(blur) = &mut self.effect {
            blur.first_frame = true;
        }
    }

    pub fn enabled(&self) -> bool {
        self.active != 0 && !matches!(&self.effect, Filter::Mosaic(mosaic) if mosaic.quad.is_none())
    }

    pub fn frame(
        &mut self,
        middle: &DrawList,
        canvas: Option<&RenderTarget>,
        targets: &mut [RenderTarget],
    ) -> Result<[DrawCommand; 2]> {
        let index = match &mut self.effect {
            Filter::Blur(blur) => {
                blur.index = 1 - blur.index;
                blur.index
            }
            Filter::Mosaic(_) => 0,
        };
        let bounds = Rect::from_xywh(0, 0, self.size[0] as i32, self.size[1] as i32);
        targets[index].clear_rect(bounds);
        middle.render_into(&mut targets[index], canvas)?;
        match &mut self.effect {
            Filter::Blur(blur) => {
                if !blur.first_frame {
                    let image = padded_image(&targets[1 - index], self.texture_size)?;
                    let quad = blur.feedback(self.size);
                    DrawList {
                        commands: quad.commands(image, self.size, None).to_vec(),
                        filter: middle.filter,
                    }
                    .render_into(&mut targets[index], None)?;
                }
                blur.first_frame = false;
                let image = padded_image(&targets[index], self.texture_size)?;
                Ok(blur.quad.commands(image, self.size, None))
            }
            Filter::Mosaic(mosaic) => {
                let (small, expanded) = mosaic.quads(self.texture_size);
                let image = padded_image(&targets[0], self.texture_size)?;
                targets[1].clear_rect(bounds);
                DrawList {
                    commands: small
                        .commands(image, self.size, Some(TextureFilter::Point))
                        .to_vec(),
                    filter: middle.filter,
                }
                .render_into(&mut targets[1], None)?;
                let image = padded_image(&targets[1], self.texture_size)?;
                Ok(expanded.commands(image, self.size, Some(TextureFilter::Point)))
            }
        }
    }
}

impl Blur {
    fn feedback(&self, size: [u32; 2]) -> Quad {
        let center = size.map(|v| -((v / 2) as f32));
        let mut quad = self.quad;
        for v in &mut quad.vertices {
            let p = crate::kcs::transform_vector(
                [
                    v.position[0] + center[0],
                    v.position[1] + center[1],
                    v.position[2],
                    v.rhw,
                ],
                self.matrix,
            );
            v.position = [
                p[0] + self.offset[0] - center[0],
                p[1] + self.offset[1] - center[1],
                p[2] + self.offset[2],
            ];
            v.rhw = p[3];
            v.color = self.color;
        }
        quad
    }
}

impl Mosaic {
    fn quads(&self, texture_size: [u32; 2]) -> (Quad, Quad) {
        let original = self.quad.unwrap();
        let mut small = original;
        let mut expanded = original;
        for (i, vertex) in small.vertices.iter_mut().enumerate() {
            for (axis, divisor) in self.divisors.into_iter().enumerate() {
                vertex.position[axis] = if divisor == 0. {
                    0.
                } else {
                    vertex.position[axis] / divisor
                };
                expanded.uv[i][axis] = vertex.position[axis] / texture_size[axis] as f32;
            }
            expanded.vertices[i].color = self.color;
        }
        (small, expanded)
    }
}

fn padded_image(target: &RenderTarget, size: [u32; 2]) -> Result<Arc<Surface>> {
    let mut image = Surface::new(size)?;
    if let Some(source) = target.tiles().first().and_then(Option::as_ref) {
        image.copy_from(source, source.bounds(), [0, 0]);
    }
    Ok(Arc::new(image))
}
