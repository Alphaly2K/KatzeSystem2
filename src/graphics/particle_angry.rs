use super::Transform;
use crate::kcs::{ParticleHost, native_triangle, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fade(from: f32, to: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    emitted_position: [f32; 3],
    position: [f32; 3],
    scale: f32,
    angle: f32,
    alpha: u32,
}

pub(super) struct Angry {
    anchor: [f32; 2],
    scale: f32,
    lives: [i32; 9],
    factor: f32,
    center: [f32; 2],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Angry {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let parameters = host.parameters.map(f32::from_bits);
        let mut scale = parameters[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        Self {
            anchor: [parameters[0], parameters[1]],
            scale,
            lives: std::array::from_fn(|i| integer(parameters[i + 3])),
            factor: if image[1] > 256. { 1. } else { 0.5 },
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            particles: host
                .quads
                .iter()
                .map(|_| Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    emitted_position: [0.; 3],
                    position: [parameters[0], parameters[1], 0.],
                    scale: scale * 0.1,
                    angle: 0.,
                    alpha: 0,
                })
                .collect(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        let Some((primary, trails)) = self.particles.split_first_mut() else {
            return;
        };
        let delta = integer(delta as f32);
        let offset = std::array::from_fn::<_, 2, _>(|i| source[i] - self.center[i]);
        let elapsed = primary.age.wrapping_sub(primary.phase_time);
        primary.position[..2].copy_from_slice(&self.anchor);
        match primary.phase {
            0 => {
                primary.scale = fade(
                    self.scale * 0.1,
                    self.scale * 1.4,
                    self.lives[0] as f32,
                    elapsed as f32,
                );
                primary.angle = fade(0., 1., self.lives[1] as f32, elapsed as f32);
                primary.alpha =
                    integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32 & 255;
            }
            1 => {
                primary.scale = fade(
                    self.scale * 1.4,
                    self.scale,
                    self.lives[1] as f32,
                    elapsed as f32,
                );
                primary.angle = fade(1., 0.3, self.lives[1] as f32, elapsed as f32);
            }
            2..=4 => {
                let displacement =
                    ((native_triangle(elapsed as f32, 4.) * self.scale) * 4.) * self.factor;
                primary.position[0] += displacement;
                if primary.phase != 2 {
                    let (from, to) = if primary.phase == 3 {
                        (self.scale, self.scale * 1.1)
                    } else {
                        (self.scale * 1.2, self.scale)
                    };
                    primary.scale = fade(
                        from,
                        to,
                        self.lives[primary.phase as usize] as f32,
                        elapsed as f32,
                    );
                }
            }
            _ => {}
        }
        if primary.phase <= 4 && elapsed > self.lives[primary.phase as usize] {
            primary.phase = if primary.phase == 4 {
                2
            } else {
                primary.phase + 1
            };
            primary.phase_time = primary.age;
        }
        if let Some(trail) = trails.iter_mut().find(|p| p.phase == 0) {
            trail.emitted_position = primary.position;
            trail.position = primary.position;
            trail.position[2] -= 1.;
            trail.angle = primary.angle;
            trail.scale = primary.scale;
            trail.phase = 1;
        }
        for trail in trails.iter_mut().filter(|p| p.phase != 0) {
            trail.position[..2].copy_from_slice(&trail.emitted_position[..2]);
            trail.alpha = integer(fade(30., 0., 29., trail.age as f32)) as u32 & 255;
            trail.scale *= 1.01;
            trail.age = trail.age.wrapping_add(delta);
            if trail.age > 29 {
                trail.alpha = 0;
                trail.age = 0;
                trail.phase = 0;
            }
            for (i, n) in offset.into_iter().enumerate() {
                trail.position[i] += n;
            }
        }
        for (i, n) in offset.into_iter().enumerate() {
            primary.position[i] += n;
        }
        primary.age = primary.age.wrapping_add(delta);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([particle.scale, particle.scale, 1.]);
            transform.set_rotation([0., 0., particle.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&particle.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        if i & 1 == 0 {
                            -self.image[0] * 0.5
                        } else {
                            self.image[0] * 0.5
                        },
                        if i & 2 == 0 {
                            -self.image[1] * 0.5
                        } else {
                            self.image[1] * 0.5
                        },
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | (particle.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn angry_keeps_phase_boundaries_trail_recycling_and_rectangular_geometry() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(30, [64., 128.]).unwrap();
        for (i, n) in [735f32, 192., 60., 4., 6., 70., 5., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut effect = Angry::new(&host, [64., 128.]);
        for step in 1u32..=127 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(
                delta,
                [
                    640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                    360. + ((step % 7) as i32 - 3) as f32 * -7.5,
                ],
            );
            let expected = match step {
                5 => [5, 0, 0, 0x4437_c000, 0x4331_0000, 0x3f57_0a3e, 0x3f2a_aaab],
                6 => [6, 5, 1, 0x4438_9000, 0x4329_8000, 0x3f57_0a3e, 0x3f55_5555],
                7 => [6, 5, 1, 0x4439_6000, 0x4356_8000, 0x3f4c_cccd, 0x3f62_2222],
                64 => [
                    98,
                    97,
                    4,
                    0x443b_0000,
                    0x434f_0000,
                    0x3f28_f5c3,
                    0x3e99_999a,
                ],
                96 => [
                    144,
                    103,
                    2,
                    0x443a_3000,
                    0x4331_0000,
                    0x3f19_999a,
                    0x3e99_999a,
                ],
                127 => [
                    195,
                    194,
                    2,
                    0x4438_9000,
                    0x434f_0000,
                    0x3f19_999a,
                    0x3e99_999a,
                ],
                _ => [0; 7],
            };
            let primary = &effect.particles[0];
            if expected != [0; 7] {
                assert_eq!(
                    [
                        primary.age as u32,
                        primary.phase_time as u32,
                        primary.phase,
                        primary.position[0].to_bits(),
                        primary.position[1].to_bits(),
                        primary.scale.to_bits(),
                        primary.angle.to_bits(),
                    ],
                    expected,
                    "step {step}"
                );
            }
            let expected = match step {
                19 => [29, 1, 1, 0x3d94_73cf],
                20 => [0, 0, 0, 0x3d95_efd9],
                21 => [0, 1, 30, 0x3f1b_22d1],
                _ => [0; 4],
            };
            if expected != [0; 4] {
                let trail = &effect.particles[1];
                assert_eq!(
                    [
                        trail.age as u32,
                        trail.phase,
                        trail.alpha,
                        trail.scale.to_bits()
                    ],
                    expected
                );
                assert_eq!(trail.position[2], -1.);
            }
            if step == 64 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                for (index, expected) in [
                    [0x4432_d5cb, 0x4371_1cae, 0x3f6c_4ec5, 0x3a83_126f],
                    [0x4432_f3bb, 0x4371_ea70, 0x3f6c_4fc7, 0x3a82_f0e9],
                ]
                .into_iter()
                .enumerate()
                {
                    let vertex = mesh.projected_quad(index, &Projection::default()).unwrap()[2];
                    assert_eq!(
                        [
                            vertex.position[0].to_bits(),
                            vertex.position[1].to_bits(),
                            vertex.position[2].to_bits(),
                            vertex.rhw.to_bits(),
                        ],
                        expected
                    );
                }
            }
        }
        host.reset_quads(0, [64., 128.]).unwrap();
        let mut empty = Angry::new(&host, [64., 128.]);
        empty.update(1, [640., 360.]);
        empty.write_mesh(&mut host);
        assert!(host.quads.is_empty());
    }
}
