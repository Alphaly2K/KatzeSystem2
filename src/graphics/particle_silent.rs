use super::Transform;
use crate::kcs::{ParticleHost, transform_vector};

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
    origin: [f32; 2],
    position: [f32; 3],
    scale: [f32; 2],
    alpha: u32,
}

pub(super) struct Silent {
    anchor: [f32; 2],
    mirror: i32,
    scale: f32,
    lives: [i32; 2],
    center: [f32; 2],
    image: [f32; 2],
    visible: usize,
    ticks: u32,
    particles: Vec<Particle>,
}

impl Silent {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let factor = if image[1] > 128. { 0.5 } else { 0.25 };
        let particles = (0..host.quads.len())
            .map(|i| {
                let origin = if i == 0 {
                    [p[0], p[1]]
                } else {
                    [
                        (p[0] - (scale * 600.) * factor) + (((i * 300) as f32 * scale) * factor),
                        (scale * -10.) * factor + p[1],
                    ]
                };
                Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    origin,
                    position: [origin[0], origin[1], i as f32],
                    scale: [scale; 2],
                    alpha: 0,
                }
            })
            .collect();
        Self {
            anchor: [p[0], p[1]],
            mirror: integer(p[2]),
            scale,
            lives: [integer(p[4]), integer(p[5])],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 1,
            ticks: 0,
            particles,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if self.particles.is_empty() {
            return;
        }
        if self.ticks > 30 {
            self.visible = (self.visible + 1).min(self.particles.len());
            self.ticks = 0;
        }
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for (i, particle) in self.particles.iter_mut().take(self.visible).enumerate() {
            particle.position[..2].copy_from_slice(&particle.origin);
            if i == 0 {
                particle.position[..2].copy_from_slice(&self.anchor);
                let time = particle.age as f32;
                match particle.phase {
                    0 => {
                        particle.scale = [2.7, 2.2].map(|to| {
                            fade(
                                self.scale * 0.01,
                                self.scale * to,
                                self.lives[0] as f32,
                                time,
                            )
                        });
                        particle.alpha =
                            integer(fade(0., 255., self.lives[0] as f32, time)) as u32 & 255;
                        if particle.age > self.lives[0] {
                            particle.phase = 1;
                            particle.phase_time = particle.age;
                        }
                    }
                    1 => {
                        // 主体第二段继续使用累计年龄，不减阶段起点。
                        particle.scale = [(2.7, 2.5), (2.2, 2.)].map(|(from, to)| {
                            fade(
                                self.scale * from,
                                self.scale * to,
                                self.lives[1] as f32,
                                time,
                            )
                        });
                        particle.alpha =
                            integer(fade(0., 255., self.lives[1] as f32, time)) as u32 & 255;
                        if particle.age > self.lives[1] {
                            particle.phase = 2;
                            particle.phase_time = particle.age;
                        }
                    }
                    _ => {
                        if particle.age > self.lives[1] {
                            particle.phase_time = particle.age;
                        }
                    }
                }
            } else {
                let elapsed = particle.age.wrapping_sub(particle.phase_time);
                match particle.phase {
                    0 => {
                        let scale = fade(
                            self.scale * 0.1,
                            self.scale * 1.2,
                            self.lives[0] as f32,
                            elapsed as f32,
                        );
                        particle.scale = [scale; 2];
                        particle.alpha =
                            integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32
                                & 255;
                        if elapsed > self.lives[0] {
                            particle.phase = 1;
                            particle.phase_time = particle.age;
                        }
                    }
                    1 => {
                        let scale = fade(
                            self.scale * 1.2,
                            self.scale * 0.7,
                            self.lives[1] as f32,
                            elapsed as f32,
                        );
                        particle.scale = [scale; 2];
                        if elapsed > self.lives[1] {
                            particle.phase = 2;
                            particle.phase_time = particle.age;
                        }
                    }
                    _ => {}
                }
            }
            for (i, value) in offset.into_iter().enumerate() {
                particle.position[i] += value;
            }
            particle.age = particle.age.wrapping_add(integer(delta as f32));
        }
        self.ticks += 1;
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (i, (particle, quad)) in self.particles.iter().zip(&mut host.quads).enumerate() {
            let mut transform = Transform::default();
            transform.set_scale([particle.scale[0], particle.scale[1], 1.]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&particle.position);
            let uv = if i == 0 {
                [0.; 2]
            } else {
                [i as f32 * 0.25, 0.75]
            };
            let size = if i == 0 { [1., 0.75] } else { [0.25; 2] };
            for (j, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        if j & 1 == 0 {
                            -self.image[0] * 0.5
                        } else {
                            self.image[0] * 0.5
                        },
                        if j & 2 == 0 {
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
                let x = if i == 0 && self.mirror != 0 {
                    1 - (j & 1)
                } else {
                    j & 1
                };
                vertex.uv = [
                    uv[0] + x as f32 * size[0],
                    uv[1] + (j >> 1) as f32 * size[1],
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn silent_keeps_update_count_activation_primary_age_and_atlas_regions() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(4, [64., 128.]).unwrap();
        for (i, value) in [782f32, 169., 0., 62., 15., 20., 2., 3., 4., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Silent::new(&host, [64., 128.]);
        for step in 1u32..=100 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            let source = [
                640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                360. + ((step % 7) as i32 - 3) as f32 * -7.5,
            ];
            effect.update(delta, source);
            let expected = match step {
                13 => Some([1, 13, 18, 16, 1, 216, 0, 0, 0x3f1e_b852, 0]),
                32 => Some([2, 1, 46, 45, 2, 255, 1, 0, 0x3d7d_f3b7, 0]),
                33 => Some([2, 2, 47, 46, 2, 255, 2, 0, 0x3ddc_177d, 17]),
                63 => Some([3, 1, 97, 90, 2, 255, 52, 2, 0x3ede_353f, 255]),
                94 => Some([4, 1, 142, 141, 2, 255, 97, 2, 0x3ede_353f, 255]),
                _ => None,
            };
            if let Some(expected) = expected {
                let primary = &effect.particles[0];
                let dot = &effect.particles[1];
                assert_eq!(
                    [
                        effect.visible as u32,
                        effect.ticks,
                        primary.age as u32,
                        primary.phase_time as u32,
                        primary.phase,
                        primary.alpha,
                        dot.age as u32,
                        dot.phase,
                        dot.scale[0].to_bits(),
                        dot.alpha
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 33 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let primary = mesh.projected_quad(0, &Projection::default()).unwrap();
                let dot = mesh.projected_quad(1, &Projection::default()).unwrap();
                assert_eq!(
                    [
                        primary[0].position[0].to_bits(),
                        primary[0].position[1].to_bits(),
                        dot[0].position[0].to_bits(),
                        dot[0].position[1].to_bits()
                    ],
                    [0x4433_099a, 0x4295_47b0, 0x4432_f8c4, 0x4311_5b84]
                );
                assert_eq!(
                    [primary[0].uv, primary[3].uv, dot[0].uv, dot[3].uv],
                    [[0., 0.], [1., 0.75], [0.25, 0.75], [0.5, 1.]]
                );
            }
        }
        host.parameters[2] = (-1f32).to_bits();
        let mut mirrored = Silent::new(&host, [64., 128.]);
        for _ in 0..32 {
            mirrored.update(0, [640., 360.]);
        }
        assert_eq!(mirrored.visible, 2);
        assert!(mirrored.particles.iter().all(|p| p.age == 0));
        mirrored.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].uv, [1., 0.]);
        assert_eq!(host.quads[1][0].uv, [0.25, 0.75]);
    }
}
