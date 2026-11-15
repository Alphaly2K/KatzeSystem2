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
    angle: f32,
    alpha: u32,
    uv: [f32; 2],
}

pub(super) struct FlowerBurst {
    anchor: [f32; 2],
    scale: f32,
    range: [f32; 2],
    spread: [f32; 2],
    lives: [f32; 5],
    center: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl FlowerBurst {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            anchor: [p[0], p[1]],
            scale: p[2] / 100.,
            range: [p[3], p[4]],
            spread: [p[5] / 100., p[6] / 100.],
            lives: std::array::from_fn(|i| p[i + 7]),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 0,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in 0..host.quads.len() {
            effect.particles.push(effect.spawn(random));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let uv = match random.next_integer() % 3 {
            0 => [0., 0.],
            1 => [0.5, 0.],
            _ => [0., 0.5],
        };
        let scale =
            (random.next_float() as f64 * self.scale as f64 + (self.scale * 0.5) as f64) as f32;
        let sign = if random.next_integer() & 1 == 0 {
            -1.
        } else {
            1.
        };
        let angle = (random.next_float() * 0.5 + 0.5) * sign;
        let mut direction =
            std::array::from_fn::<_, 2, _>(|_| (random.next_float() as f64 - 0.5) as f32);
        let length = (((direction[1] * direction[1] + direction[0] * direction[0]) + 0.) as f64)
            .sqrt() as f32;
        if length > 0. {
            direction = direction.map(|n| n / length);
        }
        let position = std::array::from_fn::<_, 2, _>(|i| {
            ((random.next_float() * (1. - self.spread[i]) + self.spread[i])
                * (self.range[i] * direction[i]))
                + self.anchor[i]
        });
        let origin = [position[0], position[1], scale + 0.];
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            scale,
            origin,
            position: origin,
            current_scale: 0.,
            angle,
            alpha: 0,
            uv,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        if random.next_integer() & 1 == 0 {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for i in 0..self.visible as usize {
            if self.particles[i].phase > 4 {
                self.particles[i] = self.spawn(random);
            } else {
                let p = &mut self.particles[i];
                let elapsed = p.age.wrapping_sub(p.phase_time) as f32;
                let duration = self.lives[p.phase as usize];
                p.position = p.origin;
                match p.phase {
                    0 => {
                        p.current_scale = fade(0., p.scale * 1.3, duration, p.age as f32);
                        p.alpha = integer(fade(0., 255., duration, p.age as f32)) as u32 & 255;
                    }
                    1 => {
                        p.angle += 0.1;
                        p.current_scale = fade(p.scale * 1.3, p.scale, duration, elapsed);
                    }
                    2 => {}
                    3 => {
                        p.current_scale = fade(p.scale * 0.8, p.scale * 1.2, duration, elapsed);
                        p.alpha = integer(fade(0., 255., duration, elapsed)) as u32 & 255;
                    }
                    4 => {
                        p.angle += 0.05;
                        p.current_scale = fade(p.scale * 1.2, p.scale * 0.1, duration, elapsed);
                        p.alpha = integer(fade(170., 0., duration, elapsed)) as u32 & 255;
                    }
                    _ => unreachable!(),
                }
                if elapsed > duration {
                    if p.phase == 2 {
                        p.uv = [0.5, 0.5];
                        p.alpha = 0;
                    }
                    p.phase += 1;
                    p.phase_time = p.age;
                }
            }
            let p = &mut self.particles[i];
            for (i, n) in source.into_iter().enumerate() {
                p.position[i] += n - self.center[i];
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.current_scale, p.current_scale, 1.]);
            transform.set_rotation([0., 0., p.angle]);
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
                vertex.uv = [
                    p.uv[0] + (i & 1) as f32 * 0.5,
                    p.uv[1] + (i >> 1) as f32 * 0.5,
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
    fn flowers_keep_fractional_stages_atlas_changes_and_respawn_randomness() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(3, [96., 300.]).unwrap();
        for (i, n) in [
            12.5f32, -8., 37.125, 13., -19., 125., -25., 1.5, 2.25, 3.5, 4.25, 5.5,
        ]
        .into_iter()
        .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = FlowerBurst::new(&host, [96., 300.], &mut random);
        assert_eq!(
            effect.particles[0].origin.map(f32::to_bits),
            [0x416d_ccb7, 0xc0a4_d044, 0x3ece_b878]
        );
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
                &mut random,
            );
            let expected = match step {
                3 => Some([3, 2, 1, 0x3f06_5e4e, 0x3f13_4297, 255, 0, 0]),
                7 => Some([6, 5, 2, 0x3ece_b878, 0x3f60_0f65, 255, 0, 0]),
                12 => Some([
                    17,
                    14,
                    3,
                    0x3ecc_49e0,
                    0x3f60_0f65,
                    120,
                    0x3f00_0000,
                    0x3f00_0000,
                ]),
                21 => Some([1, 0, 0, 0x3eb7_04ff, 0xbf21_b8f8, 170, 0, 0x3f00_0000]),
                40 => Some([3, 2, 1, 0x3f35_2183, 0xbf76_ba67, 255, 0, 0x3f00_0000]),
                127 => Some([
                    28,
                    27,
                    5,
                    0x3d40_0382,
                    0xbe52_56b9,
                    0,
                    0x3f00_0000,
                    0x3f00_0000,
                ]),
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
                        p.angle.to_bits(),
                        p.alpha,
                        p.uv[0].to_bits(),
                        p.uv[1].to_bits()
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 21 {
                let mut expected = ScriptRandom::seeded(17);
                for _ in 0..31 {
                    expected.next_integer();
                }
                for _ in 0..30 {
                    expected.next_float();
                }
                let mut actual = random.clone();
                assert_eq!(actual.next_integer(), expected.next_integer());
                assert_eq!(actual.next_float(), expected.next_float());
            }
            if step == 12 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let vertex = mesh.projected_quad(0, &Projection::default()).unwrap()[2];
                assert_eq!(vertex.uv, [0.5, 1.]);
                assert_eq!(vertex.color >> 24, 120);
                assert_eq!(
                    [
                        vertex.position[0].to_bits(),
                        vertex.position[1].to_bits(),
                        vertex.position[2].to_bits(),
                        vertex.rhw.to_bits()
                    ],
                    [0xc262_8cb0, 0x4056_cb80, 0x3f6c_4e5e, 0x3a83_1ff0]
                );
            }
        }
        host.reset_quads(0, [96., 300.]).unwrap();
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        let mut empty = FlowerBurst::new(&host, [96., 300.], &mut random);
        empty.update(0, [640., 360.], &mut random);
        expected.next_integer();
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        assert!(empty.particles.is_empty());
    }
}
