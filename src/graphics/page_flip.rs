use super::{DrawCommand, FrameTexture, ProjectedVertex, Surface, TextureSource};
use crate::{Error, Result, read};
use std::sync::Arc;

pub(crate) struct PageFlip {
    pub depth: f32,
    pub duration: u32,
    pub state: u32,
    pub now: u32,
    start: u32,
    deadline: u32,
    source: Arc<Surface>,
    vertices: Vec<PageVertex>,
    direction: f32,
    radius: f32,
    circumference: f32,
    arc_length: f32,
    end: [f32; 2],
    tangent: [f32; 2],
    minimum: f32,
    maximum: f32,
}

struct PageVertex {
    original: [f32; 2],
    base: [f32; 2],
    current: ProjectedVertex,
    uv: [f32; 2],
}

impl PageFlip {
    pub fn captured(source: Arc<Surface>) -> Result<Self> {
        let size = source.size();
        let count = u64::from(size[0].div_ceil(32)) * u64::from(size[1].div_ceil(32)) * 2;
        if count > read::MAX_ITEMS as u64 {
            return Err(Error::new(0, "翻页网格数量超过上限"));
        }
        let mut vertices = Vec::with_capacity(count as usize * 3);
        for y in (0..size[1]).step_by(32) {
            for x in (0..size[0]).step_by(32) {
                let right = x.saturating_add(32).min(size[0]);
                let bottom = y.saturating_add(32).min(size[1]);
                let corners = [[x, y], [right, y], [x, bottom], [right, bottom]];
                for i in [0, 1, 2, 1, 3, 2] {
                    let original = corners[i].map(|n| n as f32 - 0.5);
                    vertices.push(PageVertex {
                        original,
                        base: original,
                        current: ProjectedVertex {
                            position: [original[0], original[1], 0.],
                            rhw: 1.,
                            color: u32::MAX,
                        },
                        uv: std::array::from_fn(|axis| corners[i][axis] as f32 / size[axis] as f32),
                    });
                }
            }
        }
        Ok(Self {
            depth: 1_000_000.,
            duration: 0,
            state: 0,
            now: 0,
            start: 0,
            deadline: 0,
            source,
            vertices,
            direction: 0.,
            radius: 0.,
            circumference: 0.,
            arc_length: 0.,
            end: [0.; 2],
            tangent: [0.; 2],
            minimum: 0.,
            maximum: 0.,
        })
    }

    pub fn set_source(&mut self, source: Arc<Surface>) {
        self.source = source;
    }

    pub fn background(&self) -> [DrawCommand; 2] {
        let [width, height] = self.source.size().map(|n| n as f32);
        let points = [
            [-0.5, -0.5],
            [width - 0.5, -0.5],
            [-0.5, height - 0.5],
            [width - 0.5, height - 0.5],
        ];
        [[0, 1, 2], [1, 2, 3]].map(|indices| DrawCommand {
            vertices: indices.map(|i| ProjectedVertex {
                position: [points[i][0], points[i][1], 0.5],
                rhw: 1.,
                color: 0xff000000,
            }),
            texture: None,
            clip: None,
            blend_mode: 0,
        })
    }

    pub fn configure(&mut self, values: [u32; 6], now: u32, logical_width: u32) {
        self.depth = f32::from_bits(values[1]);
        self.duration = values[2];
        self.direction = f32::from_bits(values[3]);
        self.radius =
            f32::from_bits(values[4]) * (self.source.size()[0] as f32 / logical_width as f32);
        let angle = f32::from_bits(values[5]);
        self.circumference = self.radius * std::f32::consts::TAU;
        self.arc_length = (angle / std::f32::consts::TAU) * self.circumference;
        let (sine, cosine) = sin_cos(angle);
        self.end = [self.radius * sine, self.radius * cosine - self.radius];
        self.tangent = [cosine, -sine];
        if self.state != 0 {
            self.restart(now);
        }
        let center = self.source.size().map(|n| n as f32 * 0.5);
        let (sine, cosine) = sin_cos(self.direction);
        self.minimum = 100_000.;
        self.maximum = -100_000.;
        let [z, rhw, _] = self.projection(0.);
        for vertex in &mut self.vertices {
            let [x, y] = std::array::from_fn(|axis| vertex.original[axis] - center[axis]);
            vertex.base = [x * cosine + y * sine, y * cosine - x * sine];
            self.minimum = self.minimum.min(vertex.base[1]);
            self.maximum = self.maximum.max(vertex.base[1]);
            vertex.current.position[2] = z;
            vertex.current.rhw = rhw;
        }
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

    fn projection(&self, bend: f32) -> [f32; 3] {
        let origin = 65000. - (self.depth + 1000.);
        let z = origin + bend;
        let near = origin - 2000.;
        let denominator = if z == near { 1. } else { z - near };
        let scale = 2000. / denominator;
        [
            z * f32::from_bits(0x3781_0e36),
            1. / denominator,
            if scale < 0. { 0. } else { scale },
        ]
    }

    pub fn update(&mut self, now: u32) {
        self.now = now;
        if self.state == 0 {
            return;
        }
        let elapsed = now.min(self.deadline).wrapping_sub(self.start);
        let fraction = elapsed as f32 / self.deadline.wrapping_sub(self.start) as f32;
        let sweep = ((self.minimum - self.radius) - self.maximum) * fraction + self.maximum;
        let center = self.source.size().map(|n| n as f32 * 0.5);
        let (sine, cosine) = sin_cos(-self.direction);
        for i in 0..self.vertices.len() {
            let [x, y] = self.vertices[i].base;
            if y < sweep {
                continue;
            }
            let delta = y - sweep;
            let [bend_y, bend_z] = if delta < self.arc_length {
                let angle = (delta / self.circumference) * std::f32::consts::PI;
                let (sine, cosine) = sin_cos(angle + angle);
                [self.radius * sine, self.radius * cosine - self.radius]
            } else {
                let extra = delta - self.arc_length;
                std::array::from_fn(|axis| self.tangent[axis] * extra + self.end[axis])
            };
            let [z, rhw, scale] = self.projection(bend_z);
            let x = x * scale;
            let y = (bend_y + sweep) * scale;
            self.vertices[i].current = ProjectedVertex {
                position: [
                    center[0] + (x * cosine + y * sine),
                    center[1] + (y * cosine - x * sine),
                    z,
                ],
                rhw,
                color: u32::MAX,
            };
        }
    }

    pub fn frame(&mut self) -> Vec<DrawCommand> {
        let commands = self
            .vertices
            .chunks_exact(3)
            .map(|vertices| DrawCommand {
                vertices: std::array::from_fn(|i| vertices[i].current),
                texture: Some(FrameTexture {
                    source: TextureSource::Image(self.source.clone()),
                    uv: std::array::from_fn(|i| vertices[i].uv),
                }),
                clip: None,
                blend_mode: 0,
            })
            .collect();
        if self.now >= self.deadline {
            self.state = 0;
        }
        commands
    }
}

fn sin_cos(angle: f32) -> (f32, f32) {
    (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_mesh_clamps_edges_and_retains_stopped_or_reconfigured_positions() {
        let mut effect = PageFlip::captured(Arc::new(Surface::new([40, 30]).unwrap())).unwrap();
        let values = [
            0,
            16f32.to_bits(),
            8,
            0.5f32.to_bits(),
            4f32.to_bits(),
            std::f32::consts::PI.to_bits(),
        ];
        effect.configure(values, 0, 40);
        let initial = effect.frame();
        assert_eq!(initial.len(), 4);
        assert_eq!(
            initial[3].texture.as_ref().unwrap().uv,
            [[1., 0.], [1., 1.], [0.8, 1.]]
        );
        assert_eq!(initial[3].vertices[1].position[..2], [39.5, 29.5]);
        effect.set_state(7, 0);
        effect.update(4);
        let curled = effect.frame();
        assert_ne!(curled, initial);
        assert!(
            curled
                .iter()
                .flat_map(|c| c.vertices)
                .any(|v| v.rhw != 1. / 2000.)
        );
        effect.set_state(0, 4);
        effect.update(6);
        assert_eq!(effect.frame(), curled);
        effect.configure(values, 6, 40);
        let configured = effect.frame();
        for (before, after) in curled.iter().zip(&configured) {
            for (before, after) in before.vertices.iter().zip(after.vertices) {
                assert_eq!(before.position[..2], after.position[..2]);
            }
        }
        effect.set_state(3, 6);
        effect.update(14);
        assert_eq!(effect.state, 3);
        let finished = effect.frame();
        assert_eq!(effect.state, 0);
        effect.update(20);
        assert_eq!(effect.frame(), finished);
    }
}
