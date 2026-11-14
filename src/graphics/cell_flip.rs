use super::{DrawCommand, FrameTexture, ProjectedVertex, Surface, TextureSource};
use crate::{Error, Result, read};
use std::{f32::consts, sync::Arc};

pub(crate) struct CellFlip {
    pub depth: f32,
    pub state: u32,
    now: u32,
    duration: u32,
    start: u32,
    deadline: u32,
    source: [Arc<Surface>; 2],
    cells: Vec<Cell>,
    direction: f32,
    axis: f32,
    span: f32,
    minimum: f32,
    maximum: f32,
    color: u32,
}

struct Cell {
    center: [f32; 2],
    original: [[f32; 2]; 6],
    base: [[f32; 2]; 6],
    current: [[ProjectedVertex; 6]; 2],
    uv: [[f32; 2]; 6],
}

impl CellFlip {
    pub fn captured(source: [Arc<Surface>; 2]) -> Self {
        Self {
            depth: 1_000_000.,
            state: 0,
            now: 0,
            duration: 0,
            start: 0,
            deadline: 0,
            source,
            cells: Vec::new(),
            direction: 0.,
            axis: 0.,
            span: 0.,
            minimum: 0.,
            maximum: 0.,
            color: 0,
        }
    }

    pub fn set_source(&mut self, index: usize, source: Arc<Surface>) {
        self.source[index] = source;
    }

    pub fn configure(&mut self, values: [u32; 9], now: u32, logical_width: u32) -> Result<()> {
        let size = self.source[0].size();
        let scale = size[0] as f32 / logical_width as f32;
        let width = scaled_block(values[3], scale);
        // 纵向尺寸在缩放截断之后，还要乘一次原始配置值。
        let height = scaled_block(values[4], scale).wrapping_mul(values[4]);
        if width == 0 || height == 0 {
            return Err(Error::new(0, "单元翻转网格尺寸不能为零"));
        }
        let count = u64::from(size[0].div_ceil(width)) * u64::from(size[1].div_ceil(height));
        if count > read::MAX_ITEMS as u64 / 2 {
            return Err(Error::new(0, "单元翻转网格数量超过上限"));
        }
        let center = size.map(|n| n as f32 * 0.5);
        let axis = f32::from_bits(values[7]);
        let direction = f32::from_bits(values[5]) + axis;
        let span = f32::from_bits(values[6]) * scale;
        let mut minimum = 10_000f32;
        let mut maximum = -10_000f32;
        let mut cells = Vec::with_capacity(count as usize);
        for y in (0..size[1]).step_by(height as usize) {
            for x in (0..size[0]).step_by(width as usize) {
                let right = x.saturating_add(width).min(size[0]);
                let bottom = y.saturating_add(height).min(size[1]);
                let corners = [[x, y], [right, y], [x, bottom], [right, bottom]];
                let indices = [0, 1, 2, 1, 3, 2];
                let original = indices.map(|i| corners[i].map(|n| n as f32 - 0.5));
                let base = original.map(|p| {
                    let p = rotate_z([p[0] - center[0], p[1] - center[1], 0., 1.], direction);
                    [p[0], p[1]]
                });
                let point = rotate_z(
                    [
                        (x as f32 + width as f32 * 0.5) - center[0],
                        (y as f32 + height as f32 * 0.5) - center[1],
                        0.,
                        0.,
                    ],
                    direction,
                );
                minimum = minimum.min(point[1]);
                maximum = maximum.max(point[1]);
                cells.push(Cell {
                    center: [point[0], point[1]],
                    original,
                    base,
                    current: std::array::from_fn(|face| {
                        original.map(|[x, y]| ProjectedVertex {
                            position: [x, y, 0.],
                            rhw: 1.,
                            color: if face == 0 { u32::MAX } else { 0 },
                        })
                    }),
                    uv: indices.map(|i| {
                        std::array::from_fn(|axis| corners[i][axis] as f32 / size[axis] as f32)
                    }),
                });
            }
        }
        self.cells = cells;
        self.depth = f32::from_bits(values[1]);
        self.duration = values[2];
        self.direction = direction;
        self.axis = axis;
        self.span = span;
        self.minimum = minimum - span;
        self.maximum = maximum;
        self.color = values[8];
        if self.state != 0 {
            self.restart(now);
        }
        Ok(())
    }

    fn restart(&mut self, now: u32) {
        self.start = now;
        self.deadline = now.wrapping_add(self.duration);
    }

    pub fn set_state(&mut self, state: u32, now: u32) {
        self.state = state;
        if state != 0 {
            self.restart(now);
        }
    }

    pub fn update(&mut self, now: u32) {
        self.now = now;
        if self.state == 0 {
            return;
        }
        let elapsed = now.min(self.deadline).wrapping_sub(self.start);
        let fraction = elapsed as f32 / self.deadline.wrapping_sub(self.start) as f32;
        let sweep = (self.minimum - self.maximum) * fraction + self.maximum;
        let screen = self.source[0].size().map(|n| n as f32 * 0.5);
        let origin = 65000. - (self.depth + 1000.);
        let near = origin - 2000.;
        for cell in &mut self.cells {
            let delta = cell.center[1] - sweep;
            let angle = if cell.center[1] < sweep {
                0.
            } else if delta < self.span {
                (delta / self.span) * consts::PI
            } else {
                consts::PI
            };
            let (face, rotation) = if angle < consts::FRAC_PI_2 {
                (0, angle)
            } else {
                (1, angle - consts::PI)
            };
            for i in 0..6 {
                let mut point = if angle == 0. || angle >= consts::PI {
                    let [x, y] = cell.original[i];
                    [x, y, origin, 1.]
                } else {
                    let [x, y] = cell.base[i];
                    let local = [x - cell.center[0], y - cell.center[1], 0., 1.];
                    let mut point =
                        rotate_z(rotate_x(rotate_z(local, -self.axis), rotation), self.axis);
                    point[2] += origin;
                    point
                };
                let denominator = if point[2] == near {
                    1.
                } else {
                    point[2] - near
                };
                let scale = 2000. / denominator;
                let scale = if scale < 0. { 0. } else { scale };
                point[0] *= scale;
                point[1] *= scale;
                point[2] *= f32::from_bits(0x3781_0e36);
                point[3] = 1. / denominator;
                if angle != 0. && angle < consts::PI {
                    point[0] += cell.center[0];
                    point[1] += cell.center[1];
                    point = rotate_z(point, -self.direction);
                    point[0] += screen[0];
                    point[1] += screen[1];
                }
                cell.current[face][i] = ProjectedVertex {
                    position: [point[0], point[1], point[2]],
                    rhw: point[3],
                    color: u32::MAX,
                };
                cell.current[1 - face][i].color = 0;
            }
        }
    }

    pub fn clear(&self) -> [DrawCommand; 2] {
        screen(self.source[0].size(), 0xff000000, -0.5)
    }

    pub fn background(&self) -> [DrawCommand; 2] {
        screen(self.source[0].size(), self.color, 0.)
    }

    pub fn frame(&mut self) -> Vec<DrawCommand> {
        let mut commands = Vec::with_capacity(self.cells.len() * 4);
        for face in 0..2 {
            for cell in &self.cells {
                for indices in [[0, 1, 2], [3, 4, 5]] {
                    commands.push(DrawCommand {
                        vertices: indices.map(|i| cell.current[face][i]),
                        texture: Some(FrameTexture {
                            source: TextureSource::Image(self.source[face].clone()),
                            uv: indices.map(|i| cell.uv[i]),
                        }),
                        clip: None,
                        blend_mode: 0,
                    });
                }
            }
        }
        if self.now >= self.deadline {
            self.state = 0;
        }
        commands
    }
}

fn scaled_block(value: u32, scale: f32) -> u32 {
    let value = value as f32 * scale;
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        value as i32 as u32
    } else {
        i32::MIN as u32
    }
}

fn sin_cos(angle: f32) -> (f32, f32) {
    (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32)
}

fn rotate_z([x, y, z, w]: [f32; 4], angle: f32) -> [f32; 4] {
    let (sine, cosine) = sin_cos(angle);
    [x * cosine + y * sine, y * cosine - x * sine, z, w]
}

fn rotate_x([x, y, z, w]: [f32; 4], angle: f32) -> [f32; 4] {
    let (sine, cosine) = sin_cos(angle);
    [x, z * sine + y * cosine, z * cosine - y * sine, w]
}

fn screen(size: [u32; 2], color: u32, offset: f32) -> [DrawCommand; 2] {
    let [width, height] = size.map(|n| n as f32);
    let points = [
        [offset, offset],
        [width + offset, offset],
        [offset, height + offset],
        [width + offset, height + offset],
    ];
    [[0, 1, 2], [1, 3, 2]].map(|indices| DrawCommand {
        vertices: indices.map(|i| ProjectedVertex {
            position: [points[i][0], points[i][1], 0.5],
            rhw: 1.,
            color,
        }),
        texture: None,
        clip: None,
        blend_mode: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_mesh_scales_vertical_product_and_keeps_stopped_faces() {
        let image = Arc::new(Surface::new([8, 6]).unwrap());
        let mut effect = CellFlip::captured([image.clone(), image]);
        let values = [
            0,
            16f32.to_bits(),
            8,
            3,
            2,
            0,
            3f32.to_bits(),
            consts::FRAC_PI_2.to_bits(),
            0,
        ];
        effect.configure(values, 0, 8).unwrap();
        let initial = effect.frame();
        assert_eq!(initial.len(), 24);
        assert_eq!(initial[0].texture.as_ref().unwrap().uv[2], [0., 4. / 6.]);
        assert_eq!(initial[11].texture.as_ref().unwrap().uv[1], [1.; 2]);
        effect.set_state(7, 0);
        effect.update(2);
        let flipped = effect.frame();
        assert_eq!(flipped[0].vertices[0].color, 0);
        assert_eq!(flipped[12].vertices[0].color, u32::MAX);
        assert_ne!(flipped[12].vertices[0].position[2], 0.);
        effect.set_state(0, 2);
        effect.update(4);
        assert_eq!(effect.frame(), flipped);
        effect.configure(values, 4, 4).unwrap();
        let reset = effect.frame();
        assert_eq!(reset.len(), 8);
        assert_eq!(reset[0].texture.as_ref().unwrap().uv[2], [0., 1.]);
        assert!(reset.iter().flat_map(|c| c.vertices).all(|v| v.rhw == 1.));
        effect.set_state(3, 4);
        effect.update(12);
        assert_eq!(effect.state, 3);
        let final_frame = effect.frame();
        assert_eq!(effect.state, 0);
        effect.update(20);
        assert_eq!(effect.frame(), final_frame);
    }
}
