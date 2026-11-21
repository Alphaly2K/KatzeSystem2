use std::sync::Arc;

use super::{
    DrawCommand, DrawList, FrameTexture, ProjectedVertex, Rect, Surface, TextureFilter,
    TextureSource, canvas_tile_bytes,
};
use crate::{
    Error, Result,
    host::graphics::{FrameCanvas, GraphicsBackend, render_readback},
    read,
};

#[derive(Clone, Copy)]
pub(super) enum FilterKind {
    Blur,
    Mosaic,
    Shade,
}

impl FilterKind {
    pub fn accepts(self, command: u32) -> bool {
        match self {
            Self::Blur => matches!(command, 384 | 385),
            Self::Mosaic => matches!(command, 386 | 387),
            Self::Shade => matches!(command, 388 | 389),
        }
    }

    pub fn target_count(self) -> usize {
        match self {
            Self::Shade => 1,
            _ => 2,
        }
    }
}

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

struct Shade {
    quad: Quad,
    radius: Option<i32>,
}

enum Filter {
    Blur(Blur),
    Mosaic(Mosaic),
    Shade(Shade),
}

impl Filter {
    fn kind(&self) -> FilterKind {
        match self {
            Self::Blur(_) => FilterKind::Blur,
            Self::Mosaic(_) => FilterKind::Mosaic,
            Self::Shade(_) => FilterKind::Shade,
        }
    }
}

pub(super) struct ScreenFilter {
    pub active: u32,
    pub range: [f32; 2],
    size: [u32; 2],
    texture_size: [u32; 2],
    effect: Filter,
}

impl ScreenFilter {
    pub fn new(size: [u32; 2], scale: [f32; 2], kind: FilterKind) -> Self {
        // 固定采用原设备的二次幂纹理分支，使后端能力不改变取样几何。
        let texture_size = size.map(u32::next_power_of_two);
        let full_quad = || {
            Quad::rectangle(
                [0., 0., size[0] as f32, size[1] as f32],
                texture_size.map(|v| v as f32),
            )
        };
        let effect = match kind {
            FilterKind::Mosaic => Filter::Mosaic(Mosaic {
                scale,
                divisors: [1.; 2],
                color: 0,
                quad: None,
            }),
            FilterKind::Blur => Filter::Blur(Blur {
                matrix: *super::Transform::default().matrix(),
                offset: [0.; 3],
                color: 0,
                quad: full_quad(),
                first_frame: true,
                index: 0,
            }),
            FilterKind::Shade => Filter::Shade(Shade {
                quad: full_quad(),
                radius: None,
            }),
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
        let count = match self.effect {
            Filter::Blur(_) => 88,
            Filter::Mosaic(_) => 36,
            Filter::Shade(_) => 12,
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
            Filter::Shade(shade) => {
                shade.radius = Some((word(0) as i32).min(4));
                self.range = [float(1), float(2)];
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
        mut middle: DrawList,
        backend: &mut dyn GraphicsBackend,
        canvas: &mut FrameCanvas<'_>,
        targets: &mut [Surface],
        output: &mut Vec<DrawCommand>,
    ) -> Result<()> {
        if targets.len() < self.effect.kind().target_count() {
            return Err(Error::new(0, "实时滤镜的辅助渲染目标不足"));
        }
        match &mut self.effect {
            Filter::Blur(blur) => {
                if !blur.first_frame {
                    let image = padded_image(&targets[blur.index], self.texture_size)?;
                    let quad = blur.feedback(self.size);
                    middle
                        .commands
                        .extend(quad.commands(image, self.size, None));
                }
                let index = 1 - blur.index;
                let target = render_pass(backend, canvas, &middle, self.size)?;
                let image = padded_image(&target, self.texture_size)?;
                targets[index] = target;
                blur.index = index;
                blur.first_frame = false;
                output.extend(blur.quad.commands(image, self.size, None));
            }
            Filter::Mosaic(mosaic) => {
                let (small, expanded) = mosaic.quads(self.texture_size);
                let target = render_pass(backend, canvas, &middle, self.size)?;
                let image = padded_image(&target, self.texture_size)?;
                let reduction = DrawList {
                    commands: small
                        .commands(image, self.size, Some(TextureFilter::Point))
                        .to_vec(),
                    filter: middle.filter,
                };
                let reduced = render_pass(backend, canvas, &reduction, self.size)?;
                let image = padded_image(&reduced, self.texture_size)?;
                targets[0] = target;
                targets[1] = reduced;
                output.extend(expanded.commands(image, self.size, Some(TextureFilter::Point)));
            }
            Filter::Shade(shade) => {
                let mut opaque = DrawList {
                    commands: Vec::with_capacity(2 + middle.commands.len()),
                    filter: middle.filter,
                };
                opaque.push_screen_quad(
                    Rect::from_xywh(0, 0, self.size[0] as i32, self.size[1] as i32),
                    None,
                    0xff00_0000,
                    None,
                );
                opaque.commands.extend(middle.commands);
                let target = render_pass(backend, canvas, &opaque, self.size)?;
                let image = padded_image(&target, self.texture_size)?;
                targets[0] = target;
                for quad in shade.quads() {
                    output.extend(quad.commands(image.clone(), self.size, None));
                }
            }
        }
        Ok(())
    }
}

fn render_pass(
    backend: &mut dyn GraphicsBackend,
    canvas: &mut FrameCanvas<'_>,
    frame: &DrawList,
    size: [u32; 2],
) -> Result<Surface> {
    let surface = render_readback(backend, frame, canvas, size)?;
    canvas_tile_bytes(size)?;
    Ok(surface)
}

impl Shade {
    fn quads(&self) -> impl Iterator<Item = Quad> + '_ {
        const OFFSETS: [Option<[i32; 2]>; 48] = shade_offsets();
        OFFSETS.into_iter().map(|offset| {
            let mut quad = self.quad;
            if let Some([x, y]) = offset {
                for vertex in &mut quad.vertices {
                    vertex.position[0] += x as f32;
                    vertex.position[1] += y as f32;
                }
                if let Some(radius) = self.radius {
                    let [x, y, _] = quad.vertices[0].position;
                    let distance = (x * x + y * y).sqrt();
                    let opacity = if radius > 0 {
                        let radius = radius as f32;
                        ((radius - distance.min(radius)) * 255. / radius) as u32
                    } else {
                        0
                    };
                    for vertex in &mut quad.vertices {
                        vertex.color = (opacity << 24) | 0x00ff_ffff;
                    }
                }
            }
            quad
        })
    }
}

const fn shade_offsets() -> [Option<[i32; 2]>; 48] {
    let mut offsets: [Option<[i32; 2]>; 48] = [None; 48];
    let mut y: i32 = -3;
    while y <= 3 {
        let mut x: i32 = -3;
        while x <= 3 {
            let ax = x.abs();
            let ay = y.abs();
            let ring = if ax > ay { ax } else { ay };
            // 原环索引有重叠，命中同一项时依次累加偏移。
            let index = if ring == 0 {
                0
            } else {
                let side = if ay == ring {
                    if x > 0 { 5 * ring - x } else { ring + x }
                } else if y > 0 {
                    3 * ring + y
                } else {
                    7 * ring - y
                };
                (1 + 4 * ring * (ring - 1) + side) as usize
            };
            // 最后一组没有初始化顶点。
            if index < offsets.len() {
                let [u, v] = match offsets[index] {
                    Some(offset) => offset,
                    None => [0, 0],
                };
                offsets[index] = Some([u + x, v + y]);
            }
            x += 1;
        }
        y += 1;
    }
    offsets
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

fn padded_image(source: &Surface, size: [u32; 2]) -> Result<Arc<Surface>> {
    let mut image = Surface::new(size)?;
    image.copy_from(source, source.bounds(), [0, 0]);
    Ok(Arc::new(image))
}
