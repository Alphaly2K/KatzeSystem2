use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

struct Drop {
    age: i32,
    position: [f32; 3],
    velocity: [f32; 2],
    angle: f32,
    lifetime: f32,
    max_alpha: f32,
    alpha: u32,
    cell: u32,
    respawn: bool,
}

pub(super) struct Rain {
    drops: Vec<Drop>,
    lifetime: i32,
    lifetime_range: i32,
    direction: f32,
    spread: f32,
    period: u32,
    increase: u32,
    visible: u32,
    surface: [f32; 2],
    image: [f32; 2],
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value.trunc()) {
        value as i32
    } else {
        i32::MIN
    }
}

impl Rain {
    pub(super) fn new(
        particles: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Self {
        let parameters = particles.parameters.map(f32::from_bits);
        let prefill = integer(parameters[6] as f64) != 0;
        let mut rain = Self {
            drops: Vec::with_capacity(particles.quads.len()),
            lifetime: integer(parameters[0] as f64).max(1),
            lifetime_range: integer(parameters[1] as f64).max(0),
            direction: ((parameters[2] as f64 / 100.) as f32).clamp(-1., 1.),
            spread: ((parameters[3] as f64 / 100.) as f32).clamp(-0.2, 0.2),
            period: 61i32
                .wrapping_sub(integer(parameters[4] as f64))
                .clamp(1, 60) as u32,
            increase: integer(parameters[5] as f64) as u32,
            visible: if prefill {
                (particles.quads.len() >> 1) as u32
            } else {
                0
            },
            surface: particles.surface_size.map(|n| n as f32),
            image,
        };
        for i in 0..particles.quads.len() {
            let mut drop = rain.spawn(random);
            if i < rain.visible as usize {
                drop.age = integer(random.next_float() as f64 * drop.lifetime as f64);
                for axis in 0..2 {
                    drop.position[axis] = (drop.velocity[axis] as f64 * drop.age as f64
                        + drop.position[axis] as f64)
                        as f32;
                }
            }
            rain.drops.push(drop);
        }
        rain
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Drop {
        let cell = random.next_integer() & 3;
        let width = integer(self.surface[0] as f64 * 2.799_999_952_316_284) as f32;
        let x = (random.next_float() as f64 * width as f64
            - (width as f64 - self.surface[0] as f64) * 0.5) as f32;
        let z = (random.next_float() as f64 * 10.) as f32;
        let angle = (random.next_float() as f64 * self.spread as f64 - self.spread as f64 * 0.5
            + self.direction as f64) as f32;
        let lifetime =
            (random.next_float() as f64 * self.lifetime_range as f64 + self.lifetime as f64) as f32;
        let max_alpha = (random.next_integer() % 70 + 100) as f32;
        let factor = (1020. / lifetime as f64) as f32;
        // 运动方向先截到千分之一弧度；绘图旋转仍保留原始角度。
        let direction = integer(angle as f64 * 1000.) as f64 / 1000.;
        let direction = direction as f32 as f64;
        Drop {
            age: 0,
            position: [x, -100., z],
            velocity: [
                (0f32 - direction.sin() as f32) * factor,
                direction.cos() as f32 * factor,
            ],
            angle,
            lifetime,
            max_alpha,
            alpha: 0,
            cell,
            respawn: false,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.period) {
            self.visible = self.visible.wrapping_add(self.increase);
        }
        self.visible = self.visible.min(self.drops.len() as u32);
        let delta = integer(delta as f32 as f64);
        for i in 0..self.visible as usize {
            if self.drops[i].respawn {
                self.drops[i] = self.spawn(random);
            } else {
                let drop = &mut self.drops[i];
                for axis in 0..2 {
                    drop.position[axis] += drop.velocity[axis];
                }
                let fade = (drop.age as f32 / 2.).clamp(0., 1.);
                drop.alpha = integer((fade * drop.max_alpha) as f64) as u32 & 255;
                drop.respawn = drop.position[1] as f64 > self.surface[1] as f64 + 200.;
            }
            self.drops[i].age = self.drops[i].age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, particles: &mut ParticleHost) {
        for (drop, quad) in self.drops.iter().zip(&mut particles.quads) {
            let mut transform = Transform::default();
            transform.set_rotation([0., 0., drop.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&drop.position);
            let [width, height] = self.image;
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * width - width * 0.5,
                        (i >> 1) as f32 * height - height * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0x00ff_ffff | (drop.alpha << 24);
                vertex.uv = [
                    ((drop.cell & 1) + (i & 1) as u32) as f32 * 0.5,
                    ((drop.cell >> 1) + (i >> 1) as u32) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_keeps_quantized_velocity_fade_and_respawn_random_consumption() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [20f32, 10., 5., 10., 60., 5., 0.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut rain = Rain::new(&host, [64., 128.], &mut random);
        for step in 1u32..96 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            rain.update(delta, &mut random);
            let expected = match step {
                2 => [
                    0x446c_f8e8,
                    0x3fb0_9c00,
                    0x3fc0_99e4,
                    0x3d6a_711f,
                    2,
                    0xc039_1e02,
                    0x424a_c270,
                    0x3d,
                ],
                95 => [
                    0x44bc_0e52,
                    0x4428_d323,
                    0x408a_9362,
                    0x3b5e_8457,
                    30,
                    0xbdee_2c2b,
                    0x421b_0f4e,
                    0xa9,
                ],
                _ => continue,
            };
            let drop = &rain.drops[0];
            assert_eq!(
                [
                    drop.position[0].to_bits(),
                    drop.position[1].to_bits(),
                    drop.position[2].to_bits(),
                    drop.angle.to_bits(),
                    drop.age as u32,
                    drop.velocity[0].to_bits(),
                    drop.velocity[1].to_bits(),
                    drop.alpha,
                ],
                expected
            );
        }
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..103 {
            expected.next_integer();
        }
        for _ in 0..16 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
    }
}
