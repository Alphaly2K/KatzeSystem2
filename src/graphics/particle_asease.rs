use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

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
    scale: f32,
    origin: [f32; 3],
    position: [f32; 3],
    current_scale: f32,
    alpha: u32,
}

pub(super) struct Asease {
    anchor: [f32; 2],
    scale: f32,
    radius: f32,
    angle: f32,
    lives: [f32; 2],
    center: [f32; 2],
    offset: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Asease {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            anchor: [p[0], p[1]],
            scale: p[2] / 100.,
            radius: p[3],
            angle: p[4],
            lives: [p[5], p[6]],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            offset: [0.; 2],
            image,
            visible: 0,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for i in 0..host.quads.len() {
            effect.particles.push(effect.spawn(i, random));
        }
        effect
    }

    fn spawn(&self, index: usize, random: &mut ScriptRandom) -> Particle {
        let scale = (random.next_float() * self.scale) * 0.2 + self.scale * 0.8;
        // 两次寿命采样仍参与共享序列；阶段实际使用配置中的时长。
        random.next_float();
        random.next_float();
        let sign = if index.is_multiple_of(2) { 1. } else { -1. };
        let endpoint = |angle: f32| {
            let radians = (angle as f64 * std::f32::consts::PI as f64 / 180.) as f32;
            let sine = (radians as f64).sin() as f32;
            let cosine = (radians as f64).cos() as f32;
            [
                (cosine as f64 * sign as f64 - sine as f64 * 0.) as f32,
                (sine as f64 * sign as f64 + cosine as f64 * 0.) as f32,
            ]
        };
        let negative = endpoint(-self.angle);
        let positive = endpoint(self.angle);
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let delta = positive[i] - negative[i];
            ((delta as f64).powf(2.) as f32 as f64).sqrt() as f32
        });
        let x = (random.next_float() * spread[0] - spread[0] * 0.5) + sign;
        let y = (random.next_float() * spread[1] - spread[1] * 0.5) + 0.;
        let size = (((x * x + y * y) + 0.) as f64).sqrt() as f32;
        let direction = if size > 0. {
            [x / size, y / size, 0. / size]
        } else {
            [x, y, 0.]
        };
        let distance =
            (random.next_float() as f64 * self.radius as f64 + self.radius as f64) as f32;
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            scale,
            origin: [
                self.anchor[0] + direction[0] * distance,
                self.anchor[1] + direction[1] * distance,
                direction[2] * distance + 0.,
            ],
            position: [0.; 3],
            current_scale: 0.,
            alpha: 0,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        self.offset = std::array::from_fn(|i| source[i] - self.center[i]);
        if random.next_integer().is_multiple_of(2) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for i in 0..self.visible as usize {
            if self.particles[i].phase > 1 {
                let previous_position = self.particles[i].position;
                self.particles[i] = self.spawn(i, random);
                self.particles[i].position = previous_position;
            } else {
                let p = &mut self.particles[i];
                p.position = p.origin;
                let time = if p.phase == 0 {
                    p.age
                } else {
                    p.age.wrapping_sub(p.phase_time)
                } as f32;
                p.current_scale = if p.phase == 0 {
                    p.alpha = integer(fade(0., 255., self.lives[0], time)) as u32 & 255;
                    fade(0., p.scale, self.lives[0], time)
                } else {
                    fade(p.scale * 1.3, p.scale, self.lives[1], time)
                };
                if time > self.lives[p.phase as usize] {
                    p.phase = if p.phase == 0 { 1 } else { 10 };
                    p.phase_time = p.age;
                }
            }
            let p = &mut self.particles[i];
            for (i, offset) in self.offset.into_iter().enumerate() {
                p.position[i] += offset;
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.current_scale, p.current_scale, 1.]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
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
                vertex.color = 0xffffff | (p.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn asease_keeps_activation_phase_boundaries_respawn_and_random_rounding() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(2, [64., 128.]).unwrap();
        for (i, value) in [782f32, 169., 62., 30., 15., 10., 20.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Asease::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.particles[0].scale.to_bits(), 0x3f11_a086);
        assert_eq!(
            effect.particles[0].origin.map(f32::to_bits),
            [0x444d_1c54, 0x431f_5665, 0]
        );
        assert!(effect.particles[1].origin[0] < effect.anchor[0]);
        for step in 1u32..=40 {
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
            effect.update(delta, source, &mut random);
            let expected = match step {
                7 => Some([6, 0, 0, 0x3eae_c0a1, 0x99]),
                11 => Some([16, 14, 1, 0x3f3b_2179, 255]),
                27 => Some([42, 35, 10, 0x3f11_a086, 255]),
                28 => Some([0, 0, 0, 0, 0]),
                30 => Some([2, 0, 0, 0x3d70_d7fa, 25]),
                _ => None,
            };
            if let Some(expected) = expected {
                let p = &effect.particles[0];
                assert_eq!(
                    [
                        p.age as u32,
                        p.phase_time as u32,
                        p.phase,
                        p.current_scale.to_bits(),
                        p.alpha
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 11 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let v = mesh.projected_quad(0, &Projection::default()).unwrap()[0];
                assert_eq!(
                    [
                        v.position[0].to_bits(),
                        v.position[1].to_bits(),
                        v.position[2].to_bits(),
                        v.rhw.to_bits()
                    ],
                    [0x4443_3348, 0x42d2_1c0e, 0x3f6c_4ec5, 0x3a83_126f]
                );
                assert_eq!((v.color, v.uv), (u32::MAX, [0.; 2]));
            }
        }
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..40 {
            expected.next_integer();
        }
        for _ in 0..24 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
    }
}
