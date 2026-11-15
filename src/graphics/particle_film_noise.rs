use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    life: f32,
    position: [f32; 3],
    scale: [f32; 3],
    angle: f32,
    colors: [u32; 4],
    enabled: bool,
}

pub(super) struct FilmNoise {
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl FilmNoise {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        Self {
            image,
            visible: 3,
            particles: (0..host.quads.len())
                .map(|i| Self::spawn(i, random))
                .collect(),
        }
    }

    fn spawn(index: usize, random: &mut ScriptRandom) -> Particle {
        let mut p = Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            life: 0.,
            position: [640., 360., 20.],
            scale: [1.25, 0.8, 0.],
            angle: 0.,
            colors: [0x05ff_ffff; 4],
            enabled: false,
        };
        match index {
            0 => p.phase = 1,
            1 => {}
            2 => {
                p.position = [random.next_float() * 1280., 200., 10.];
                p.scale = [((random.next_float() * 0.1) as f64 + 0.1) as f32, 1., 1.];
                p.colors = std::array::from_fn(|_| 0xffffff | (random.next_integer() << 24));
                p.life = (random.next_integer() % 5 + 1) as f32;
                p.enabled = true;
            }
            _ => {
                p.position = [
                    random.next_float() * 1280.,
                    random.next_float() * 720.,
                    (index - 2) as f32,
                ];
                p.angle = random.next_float() * std::f32::consts::PI;
                let scale = (random.next_float() as f64 * 0.01f32 as f64 + 0.005f32 as f64) as f32;
                p.scale = [scale, scale, 1.];
                p.colors =
                    std::array::from_fn(|_| 0xffffff | ((random.next_integer() % 200) << 24));
                p.life = (random.next_integer() & 7) as f32;
            }
        }
        p
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(30) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for (i, p) in self
            .particles
            .iter_mut()
            .take(self.visible as usize)
            .enumerate()
        {
            let elapsed = p.age.wrapping_sub(p.phase_time);
            if i < 2 {
                p.enabled = if p.phase == 0 { i == 0 } else { i == 1 };
                if elapsed > 1 {
                    p.phase ^= 1;
                    p.phase_time = p.age;
                }
            } else if p.phase != 0 {
                *p = Self::spawn(i, random);
            } else {
                if i != 2 {
                    p.enabled = true;
                }
                if elapsed as f32 > p.life {
                    p.phase = 1;
                    p.phase_time = p.age;
                }
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (i, (p, quad)) in self.particles.iter().zip(&mut host.quads).enumerate() {
            let mut transform = Transform::default();
            transform.set_scale(p.scale);
            transform.set_rotation([0., 0., p.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            let (uv, size) = match i {
                0 => ([0., 0.], [0.625, 0.3125]),
                1 => ([0., 0.5], [0.625, 0.3125]),
                2 => ([0.75, 0.5], [0.25, 0.5]),
                _ => ([0.75, 0.], [0.25, 0.25]),
            };
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
                vertex.color = p.colors[j];
                vertex.uv = [
                    uv[0] + (j & 1) as f32 * size[0],
                    uv[1] + (j >> 1) as f32 * size[1],
                ];
            }
            host.quad_flags[i] = u32::from(p.enabled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn film_noise_keeps_flicker_activation_regeneration_and_random_rounding() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(8, [64., 128.]).unwrap();
        let mut random = ScriptRandom::seeded(17);
        let mut effect = FilmNoise::new(&host, [64., 128.], &mut random);
        assert_eq!(
            effect.particles[3].colors,
            [0x4eff_ffff, 0x1eff_ffff, 0x17ff_ffff, 0xa8ff_ffff]
        );
        for step in 1u32..=159 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, &mut random);
            let expected = match step {
                4 => Some([3, 7, 4, 0, 0x4040_0000, 0, 0, 0x3c06_8ee7]),
                7 => Some([3, 4, 1, 0, 0x3f80_0000, 0, 0, 0x3c06_8ee7]),
                36 => Some([4, 15, 8, 0, 0x40a0_0000, 7, 0, 0x3c06_8ee7]),
                64 => Some([4, 7, 11, 1, 0x4080_0000, 1, 0, 0x3c3d_01e5]),
                100 => Some([7, 127, 8, 1, 0x3f80_0000, 8, 1, 0x3c09_4ec2]),
                159 => Some([8, 183, 5, 0, 0x4080_0000, 1, 0, 0x3c1e_9e0a]),
                _ => None,
            };
            if let Some(expected) = expected {
                let scratch = &effect.particles[2];
                let speck = &effect.particles[3];
                let enabled = effect
                    .particles
                    .iter()
                    .enumerate()
                    .fold(0, |mask, (i, p)| mask | (u32::from(p.enabled) << i));
                assert_eq!(
                    [
                        effect.visible,
                        enabled,
                        scratch.age as u32,
                        scratch.phase,
                        scratch.life.to_bits(),
                        speck.age as u32,
                        speck.phase,
                        speck.scale[0].to_bits()
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 36 {
                effect.write_mesh(&mut host);
                assert_eq!(host.quad_flags, [1, 1, 1, 1, 0, 0, 0, 0]);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let vertex = mesh.projected_quad(3, &Projection::default()).unwrap()[2];
                assert_eq!(vertex.color, 0x17ff_ffff);
                assert_eq!(vertex.uv, [0.75, 0.25]);
                assert_eq!(
                    [
                        vertex.position[0].to_bits(),
                        vertex.position[1].to_bits(),
                        vertex.position[2].to_bits(),
                        vertex.rhw.to_bits()
                    ],
                    [0x4437_1bf6, 0x40d3_bcc0, 0x3f6c_4dc3, 0x3a83_3405]
                );
            }
        }
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..789 {
            expected.next_integer();
        }
        for _ in 0..436 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
    }
}
