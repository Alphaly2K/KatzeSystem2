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

#[derive(Clone)]
struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    target: [f32; 3],
    jitter: [f32; 3],
    position: [f32; 3],
    scale: f32,
    rotation: f32,
    flip: u32,
    alpha: u32,
}

pub(super) struct Dis {
    anchor: [f32; 3],
    center: [f32; 2],
    scale: f32,
    lives: [i32; 4],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Dis {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1], 0.];
        let mirror = integer(p[2]) != 0;
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let angle = if mirror { 0.7f32 } else { -0.7 };
        let angle = integer(angle * 1000.) as f32 / 1000.;
        let sine = (angle as f64).sin() as f32;
        let cosine = (angle as f64).cos() as f32;
        let direction = [
            ((cosine * 0. - (-sine * 1.)) + 0.) + 0.,
            ((sine * 0. - cosine * 1.) + 0.) + 0.,
            0.,
        ];
        let target = std::array::from_fn(|j| ((direction[j] * scale) * 50.) + anchor[j]);
        let particles = vec![
            Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                target,
                jitter: [0.; 3],
                position: anchor,
                scale,
                rotation: 0.,
                flip: u32::from(mirror),
                alpha: 0,
            };
            host.quads.len()
        ];
        Self {
            anchor,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            scale,
            lives: std::array::from_fn(|i| integer(p[i + 4])),
            image,
            particles,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for (index, p) in self.particles.iter_mut().enumerate() {
            let elapsed = p.age.wrapping_sub(p.phase_time);
            match p.phase {
                0 | 1 => {
                    let duration = self.lives[p.phase as usize] as f32;
                    for j in 0..2 {
                        let (from, to) = if p.phase == 0 {
                            (self.anchor[j], p.target[j])
                        } else {
                            (p.target[j], self.anchor[j])
                        };
                        p.position[j] = fade(from, to, duration, elapsed as f32);
                    }
                    let (from_scale, to_scale, from_alpha, to_alpha) = if p.phase == 0 {
                        (self.scale, self.scale * 1.2, 0., 190.)
                    } else {
                        (self.scale * 1.2, self.scale, 190., 255.)
                    };
                    p.scale = fade(from_scale, to_scale, duration, elapsed as f32);
                    p.alpha =
                        integer(fade(from_alpha, to_alpha, duration, elapsed as f32)) as u32 & 255;
                    if elapsed > self.lives[p.phase as usize] {
                        p.phase += 1;
                        p.phase_time = p.age;
                    }
                }
                2 => {
                    p.jitter = self.anchor;
                    p.position[..2].copy_from_slice(&self.anchor[..2]);
                    if index == 1 {
                        for j in 0..2 {
                            p.jitter[j] = (random.next_float() * 6. - 3.) + self.anchor[j];
                        }
                        p.position[..2].copy_from_slice(&p.jitter[..2]);
                        // 第三个浮点随机值未用于属性，仍需保留序列消费。
                        random.next_float();
                        p.rotation = (f64::from(random.next_float())
                            * f64::from(std::f32::consts::TAU))
                            as f32;
                        p.flip = random.next_integer() & 3;
                    }
                    p.phase = 3;
                    p.phase_time = p.age;
                }
                3 => {
                    p.position[..2].copy_from_slice(&p.jitter[..2]);
                    if elapsed > self.lives[3] {
                        p.phase = 2;
                        p.phase_time = p.age;
                    }
                }
                _ => {
                    p.position[..2].copy_from_slice(&self.anchor[..2]);
                }
            }
            for (i, n) in offset.into_iter().enumerate() {
                p.position[i] += n;
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (index, (p, quad)) in self.particles.iter().zip(&mut host.quads).enumerate() {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., 0., p.rotation]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, v) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (i >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = 0x00ff_ffff | (p.alpha << 24);
                let x = if p.flip & 1 != 0 { 1 - (i & 1) } else { i & 1 };
                let y = if p.flip & 2 != 0 {
                    1 - (i >> 1)
                } else {
                    i >> 1
                };
                v.uv = [(usize::from(index != 0) + x) as f32 * 0.5, y as f32 * 0.5];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dis_keeps_phase_boundaries_jitter_streams_and_texture_flips() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(5, [64., 128.]).unwrap();
        for (i, value) in [1161f32, 16., 1., 40., 5., 8., 1., 10.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Dis::new(&host, [64., 128.]);
        assert_eq!(effect.particles[0].target[0].to_bits(), 0x4492_bc4d);
        assert_eq!(effect.particles[0].target[1].to_bits(), 0x3f34_01f0);
        let mut random = ScriptRandom::seeded(17);
        let mut expected_random = random.clone();
        let mut previous_calls = 0;
        for step in 1u32..=70 {
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
            let (head, jitter, calls) = match step {
                7 => ([6, 6, 1, 190], [0x4493_8c4d, 0x41b9_a010, 0, 1], 0),
                17 => (
                    [21, 16, 3, 255],
                    [0x4491_98cf, 0x415e_71f1, 0x3d66_cd30, 0],
                    1,
                ),
                40 => (
                    [59, 56, 3, 255],
                    [0x4492_1e73, 0x3f19_29e0, 0x407d_d550, 1],
                    4,
                ),
                70 => (
                    [103, 98, 3, 255],
                    [0x4490_b447, 0x4215_34a4, 0x403d_95a6, 2],
                    7,
                ),
                _ => continue,
            };
            let p = &effect.particles[0];
            assert_eq!([p.age as u32, p.phase_time as u32, p.phase, p.alpha], head);
            let p = &effect.particles[1];
            assert_eq!(
                [
                    p.position[0].to_bits(),
                    p.position[1].to_bits(),
                    p.rotation.to_bits(),
                    p.flip
                ],
                jitter
            );
            assert_eq!(effect.particles[2].rotation, 0.);
            for _ in previous_calls..calls {
                expected_random.next_integer();
                for _ in 0..4 {
                    expected_random.next_float();
                }
            }
            previous_calls = calls;
            let mut actual = random.clone();
            let mut expected = expected_random.clone();
            assert_eq!(actual.next_integer(), expected.next_integer());
            assert_eq!(actual.next_float(), expected.next_float());
        }
    }
}
