use super::{DrawCommand, FrameTexture, ProjectedVertex, Surface, TextureSource};
use crate::kcs::{multiply_matrix, transform_vector};
use crate::{Error, Result, read};
use std::{collections::BTreeMap, sync::Arc};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Default)]
pub(crate) struct Shatters {
    pub entries: BTreeMap<u32, Shatter>,
}

pub(crate) struct Shatter {
    pub far: f32,
    pub duration: u32,
    pub state: u32,
    pub now: u32,
    pub start: u32,
    pub deadline: u32,
    previous: u32,
    pub mesh: Option<ShatterMesh>,
    pub source: Option<Arc<Surface>>,
}

impl Default for Shatter {
    fn default() -> Self {
        Self {
            far: 1_000_000.,
            duration: 0,
            state: 0,
            now: 0,
            start: 0,
            deadline: 0,
            previous: 0,
            mesh: None,
            source: None,
        }
    }
}

pub(crate) struct ShatterMesh {
    pub size: [u32; 2],
    fragments: Vec<Fragment>,
}

struct Fragment {
    original: [[f32; 4]; 3],
    current: [[f32; 4]; 3],
    uv: [[f32; 2]; 3],
    center: [f32; 3],
    position: [f32; 3],
    velocity: [f32; 3],
    angular_velocity: [f32; 3],
    rotation: [f32; 16],
    color: u32,
}

impl ShatterMesh {
    pub fn new(
        size: [u32; 2],
        block: [u32; 2],
        speed: f32,
        spin: f32,
        mut random: impl FnMut() -> f32,
    ) -> Result<Self> {
        if block.contains(&0) {
            return Err(Error::new(0, "碎裂网格尺寸不能为零"));
        }
        let count = u64::from(size[0].div_ceil(block[0]))
            .checked_mul(u64::from(size[1].div_ceil(block[1])))
            .and_then(|count| count.checked_mul(2))
            .filter(|&count| count <= read::MAX_ITEMS as u64)
            .ok_or_else(|| Error::new(0, "碎裂网格数量超过上限"))?;
        let mut fragments = Vec::with_capacity(count as usize);
        let screen_center = size.map(|n| n as f32 * 0.5);
        let radius =
            (screen_center[1] * screen_center[1] + screen_center[0] * screen_center[0]).sqrt();
        for y in (0..size[1]).step_by(block[1] as usize) {
            for x in (0..size[0]).step_by(block[0] as usize) {
                let corners = [
                    [x, y],
                    [x.wrapping_add(block[0]), y],
                    [x, y.wrapping_add(block[1])],
                    [x.wrapping_add(block[0]), y.wrapping_add(block[1])],
                ];
                for indices in [[0, 1, 2], [1, 2, 3]] {
                    let original = indices.map(|i| {
                        [
                            corners[i][0] as f32 - 0.5,
                            corners[i][1] as f32 - 0.5,
                            0.,
                            1.,
                        ]
                    });
                    let uv = indices.map(|i| {
                        [
                            corners[i][0] as f32 / size[0] as f32,
                            corners[i][1] as f32 / size[1] as f32,
                        ]
                    });
                    let center: [f32; 3] = std::array::from_fn(|i| {
                        ((original[0][i] + original[1][i]) + original[2][i]) / 3.
                    });
                    let offset = [center[0] - screen_center[0], center[1] - screen_center[1]];
                    let distance = (offset[1] * offset[1] + offset[0] * offset[0]).sqrt();
                    let velocity = [0., 0., random() * speed, 0.];
                    let matrix =
                        rotate(IDENTITY, 0, distance / radius * std::f32::consts::FRAC_PI_4);
                    let angle = f64::from(offset[0]).atan2(-f64::from(offset[1])) as f32;
                    let velocity = transform_vector(velocity, rotate(matrix, 2, angle));
                    let full = (spin * std::f32::consts::PI) / 180.;
                    let half = ((spin * 0.5) * std::f32::consts::PI) / 180.;
                    let angular_velocity = std::array::from_fn(|_| {
                        (f64::from(half) - f64::from(random()) * f64::from(full)) as f32
                    });
                    fragments.push(Fragment {
                        original,
                        current: original,
                        uv,
                        center: [
                            offset[0] + screen_center[0],
                            offset[1] + screen_center[1],
                            center[2],
                        ],
                        position: center,
                        velocity: [velocity[0], velocity[1], 0.],
                        angular_velocity,
                        rotation: IDENTITY,
                        color: u32::MAX,
                    });
                }
            }
        }
        Ok(Self { size, fragments })
    }
}

impl Shatter {
    pub fn captured(source: Arc<Surface>) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }

    pub fn background(&self) -> Option<[DrawCommand; 2]> {
        let [width, height] = self.source.as_ref()?.size().map(|n| n as f32);
        let points = [
            [-0.5, -0.5],
            [width - 0.5, -0.5],
            [-0.5, height - 0.5],
            [width - 0.5, height - 0.5],
        ];
        Some([[0, 1, 2], [1, 2, 3]].map(|indices| DrawCommand {
            vertices: indices.map(|i| ProjectedVertex {
                position: [points[i][0], points[i][1], 0.5],
                rhw: 1.,
                color: 0xff000000,
            }),
            texture: None,
            clip: None,
            blend_mode: 0,
        }))
    }

    pub fn restart(&mut self, now: u32) {
        self.start = now;
        self.deadline = now.wrapping_add(self.duration);
    }

    pub fn set_state(&mut self, value: u32, now: u32) {
        self.state = value;
        if value != 0 {
            self.restart(now);
            self.previous = 0;
        }
    }

    pub fn frame(&mut self) -> Vec<DrawCommand> {
        let (Some(mesh), Some(source)) = (&mut self.mesh, &self.source) else {
            return Vec::new();
        };
        if self.state != 0 {
            let elapsed = self.now.min(self.deadline).wrapping_sub(self.start);
            let delta = elapsed.wrapping_sub(self.previous) as f32;
            let color = ((255 - (elapsed.wrapping_mul(255) / self.duration.max(1)).min(255)) << 24)
                | 0xffffff;
            for fragment in &mut mesh.fragments {
                for axis in 0..3 {
                    fragment.rotation = rotate(
                        fragment.rotation,
                        axis,
                        fragment.angular_velocity[axis] * delta,
                    );
                    fragment.position[axis] += fragment.velocity[axis] * delta;
                }
                fragment.current = fragment.original.map(|mut point| {
                    for (value, center) in point.iter_mut().zip(fragment.center) {
                        *value -= center;
                    }
                    let mut point = transform_vector(point, fragment.rotation);
                    for (value, position) in point.iter_mut().zip(fragment.position) {
                        *value += position;
                    }
                    point[3] = 1.;
                    point
                });
                fragment.color = color;
            }
            self.previous = elapsed;
        }
        let commands = mesh
            .fragments
            .iter()
            .map(|fragment| DrawCommand {
                vertices: fragment
                    .current
                    .map(|point| project(point, mesh.size, fragment.color)),
                texture: Some(FrameTexture {
                    source: TextureSource::Image(source.clone()),
                    uv: fragment.uv,
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

fn rotate(matrix: [f32; 16], axis: usize, angle: f32) -> [f32; 16] {
    let (sine, cosine) = super::transform::sin_cos(angle);
    let mut rotation = IDENTITY;
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    rotation[a * 4 + a] = cosine;
    rotation[b * 4 + b] = cosine;
    rotation[a * 4 + b] = sine;
    rotation[b * 4 + a] = -sine;
    multiply_matrix(matrix, rotation)
}

fn project([mut x, mut y, z, _]: [f32; 4], size: [u32; 2], color: u32) -> ProjectedVertex {
    let depth = 65000. - (z + 32500.);
    let (z, rhw) = if z != 0. {
        let denominator = if depth == 31500. { 1. } else { depth - 31500. };
        let scale = 1000. / denominator;
        let scale = if scale < 0. { 0. } else { scale };
        x = (x - size[0] as f32) * scale + size[0] as f32;
        y = (y - size[1] as f32) * scale + size[1] as f32;
        (depth * f32::from_bits(0x3781_0e36), 1. / denominator)
    } else {
        (depth * f32::from_bits(0x3781_0e36), 1.)
    };
    ProjectedVertex {
        position: [x, y, z],
        rhw,
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_keeps_oversized_edge_cells_and_draws_each_elapsed_delta_once() {
        let mut samples = 0;
        let mesh = ShatterMesh::new([8, 6], [3, 4], 2., 30., || {
            samples += 1;
            0.25
        })
        .unwrap();
        assert_eq!(mesh.fragments.len(), 12);
        assert_eq!(samples, 48);
        assert_eq!(mesh.fragments[11].uv[2], [9. / 8., 8. / 6.]);
        let mut effect = Shatter {
            duration: 8,
            mesh: Some(mesh),
            source: Some(Arc::new(Surface::new([8, 6]).unwrap())),
            ..Default::default()
        };
        effect.set_state(7, 0);
        effect.now = 2;
        let frame = effect.frame();
        assert!(frame.iter().flat_map(|c| c.vertices).any(|v| v.rhw != 1.));
        assert_eq!(effect.frame(), frame);
        effect.set_state(0, 2);
        effect.now = 6;
        assert_eq!(effect.frame(), frame);
        effect.set_state(3, 6);
        effect.now = 14;
        assert!(
            effect
                .frame()
                .iter()
                .flat_map(|c| c.vertices)
                .all(|v| v.color >> 24 == 0)
        );
        assert_eq!(effect.state, 0);
        assert!(ShatterMesh::new([u32::MAX; 2], [1; 2], 0., 0., || 0.).is_err());
    }
}
