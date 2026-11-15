use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(n: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&(n as f64)) {
        n as i32
    } else {
        i32::MIN
    }
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let q = v.map(|n| n * n);
    let len = (((q[0] + q[1]) + q[2]) as f64).sqrt() as f32;
    if len > 0. { v.map(|n| n / len) } else { v }
}

fn linear(from: f32, to: f32, end: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= end {
        to
    } else {
        (time / end) * (to - from) + from
    }
}

struct Sprite {
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    color: u32,
    region: u32,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    rotation_step: [f32; 3],
    sprite: Sprite,
}

pub(super) struct Spark {
    line: Path,
    image: [f32; 2],
    scale: f32,
    step: f32,
    lives: [i32; 3],
    direction: [f32; 2],
    direction_mode: i32,
    spread: f32,
    frequency: u32,
    increment: u32,
    active: usize,
    mode: u32,
    rotation: [f32; 6],
    heads: Vec<Head>,
}

impl Spark {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            line: Path::new(vec![[p[0], p[1], 0.], [p[2], p[3], 0.]], 100., 1.),
            image,
            scale: if p[4] / 100. < 0.01 {
                0.01
            } else {
                p[4] / 100.
            },
            step: p[5],
            lives: [p[6], p[7], p[8]].map(|n| integer(n).max(0)),
            direction: [p[9] / 100., p[10] / 100.],
            direction_mode: integer(p[11]),
            spread: p[12] / 100.,
            frequency: 61i32.wrapping_sub(integer(p[13])).clamp(1, 60) as u32,
            increment: integer(p[14]) as u32,
            active: if integer(p[15]) != 0 {
                host.quads.len() / 2
            } else {
                1
            },
            mode: if integer(p[16]) != 0 {
                1
            } else {
                host.render_mode
            },
            rotation: [p[17], p[18], p[19], p[20], p[21], p[22]].map(|n| n / 100.),
            heads: Vec::with_capacity(host.quads.len()),
        };
        for i in 0..host.quads.len() {
            let mut head = effect.spawn(random, 0, [0.; 2]);
            if integer(p[15]) != 0 && i < effect.active {
                let total = effect.lives.into_iter().fold(0i32, i32::wrapping_add);
                head.age = integer((random.next_float() as f64 * total as f32 as f64) as f32);
                if head.age > effect.lives[0] {
                    head.phase += 1;
                    head.phase_time = head.phase_time.wrapping_add(effect.lives[0]);
                    head.sprite.color |= 0xff00_0000;
                }
                if head.age > effect.lives[1] {
                    head.phase += 1;
                    head.phase_time = head.phase_time.wrapping_add(effect.lives[1]);
                }
            }
            effect.heads.push(head);
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom, phase_time: i32, rotation: [f32; 2]) -> Head {
        let region = random.next_integer() & 3;
        let size = (random.next_float() as f64 * self.scale as f64 + self.scale as f64) as f32;
        let start = self.line.at((random.next_float() as f64 * 100.) as f32);
        let angle = (random.next_float() as f64 * std::f32::consts::PI as f64) as f32;
        let rotation_step = [2, 4, 0].map(|i| {
            (random.next_float() * self.rotation[i + 1] + self.rotation[i])
                - self.rotation[i + 1] * 0.5
        });
        let x = (random.next_float() as f64 * self.direction[1] as f64 - self.direction[0] as f64)
            as f32;
        let r = random.next_float();
        let y = match self.direction_mode {
            1 => -(r * self.spread + 0.05),
            2 => (r as f64 * self.spread as f64 - (self.spread * 0.5) as f64) as f32,
            _ => (r as f64 * self.spread as f64 + 0.05f32 as f64) as f32,
        };
        let direction = normalized([x, y, 0.]);
        let distance = (random.next_float() as f64 * self.step as f64 + self.step as f64) as f32;
        let mut point = std::array::from_fn(|i| start[i] + direction[i] * distance);
        let direction = normalized(std::array::from_fn(|i| point[i] - start[i]));
        let mut points = vec![start, point];
        let bound_angle = (50f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let sin = (bound_angle as f64).sin() as f32;
        let cos = (bound_angle as f64).cos() as f32;
        // 边界首项乘积先写回单精度，差值再使用较宽的中间精度。
        let low = [
            ((cos * direction[0]) as f64 + sin as f64 * direction[1] as f64) as f32,
            ((cos * direction[1]) as f64 - sin as f64 * direction[0] as f64) as f32,
        ];
        let high = [
            ((cos * direction[0]) as f64 - sin as f64 * direction[1] as f64) as f32,
            ((cos * direction[1]) as f64 + sin as f64 * direction[0] as f64) as f32,
        ];
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let d = high[i] - low[i];
            ((d as f64 * d as f64) as f32 as f64).sqrt() as f32
        });
        for _ in 0..4 {
            let perturb =
                spread.map(|s| (random.next_float() as f64 * s as f64 - (s * 0.5) as f64) as f32);
            let dir = normalized([direction[0] + perturb[0], direction[1] + perturb[1], 0.]);
            let distance =
                (random.next_float() as f64 * self.step as f64 + self.step as f64) as f32;
            point = std::array::from_fn(|i| point[i] + dir[i] * distance);
            points.push(point);
            if random.next_integer().is_multiple_of(15) {
                break;
            }
        }
        let total = self.lives.into_iter().fold(0i32, i32::wrapping_add);
        Head {
            age: 0,
            phase_time,
            phase: 0,
            path: Path::new(points, total as f32, self.step * 1.25),
            rotation_step,
            sprite: Sprite {
                position: start,
                rotation: [rotation[0], rotation[1], angle],
                scale: [size, size, 1.],
                color: 0xffffff,
                region,
            },
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.frequency) {
            self.active = (self.active as u32).wrapping_add(self.increment) as usize;
        }
        self.active = self.active.min(self.heads.len());
        for i in 0..self.active {
            if self.heads[i].phase > 2 {
                self.heads[i] = self.spawn(
                    random,
                    self.heads[i].phase_time,
                    [
                        self.heads[i].sprite.rotation[0],
                        self.heads[i].sprite.rotation[1],
                    ],
                );
            } else {
                let h = &mut self.heads[i];
                h.sprite.position = h.path.at(h.age as f32);
                for axis in 0..3 {
                    h.sprite.rotation[axis] += h.rotation_step[axis];
                }
                let elapsed = if h.phase == 0 {
                    h.age
                } else {
                    h.age.wrapping_sub(h.phase_time)
                };
                let alpha = match h.phase {
                    0 => Some(integer(linear(
                        0.,
                        255.,
                        self.lives[0] as f32,
                        elapsed as f32,
                    ))),
                    2 => Some(integer(linear(
                        255.,
                        0.,
                        self.lives[2] as f32,
                        elapsed as f32,
                    ))),
                    _ => None,
                };
                if let Some(alpha) = alpha {
                    h.sprite.color = (h.sprite.color & 0xffffff) | ((alpha as u32 & 255) << 24);
                }
                if elapsed > self.lives[h.phase as usize] {
                    h.phase += 1;
                    h.phase_time = h.age;
                }
            }
            self.heads[i].age = self.heads[i].age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (i, h) in self.heads.iter().enumerate() {
            let s = &h.sprite;
            let mut transform = Transform::default();
            transform.set_scale(s.scale);
            transform.set_rotation(s.rotation);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&s.position);
            for (j, v) in host.quads[i].iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (j & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (j >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = s.color;
                v.uv = [
                    ((s.region & 1) + (j & 1) as u32) as f32 * 0.5,
                    ((s.region >> 1) + (j >> 1) as u32) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_counts_zero_steps_and_phase_changes_use_strict_lifetime_boundaries() {
        let mut host = ParticleHost::default();
        host.reset_quads(3, [64., 128.]).unwrap();
        for (i, n) in [
            125f32, -70., 530., 410., 65., 15., 3., 8., 5., 20., 70., 2., 80., 60., 1.,
        ]
        .into_iter()
        .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Spark::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.active, 1);
        effect.update(0, &mut random);
        assert_eq!(effect.active, 2);
        effect.update(3, &mut random);
        assert_eq!(effect.active, 3);
        assert_eq!(effect.heads[0].phase, 0);
        effect.update(1, &mut random);
        assert_eq!(effect.heads[0].phase, 0);
        effect.update(0, &mut random);
        assert_eq!(effect.heads[0].phase, 1);
        effect.update(9, &mut random);
        effect.update(0, &mut random);
        assert_eq!(effect.heads[0].phase, 2);
        effect.update(6, &mut random);
        effect.update(0, &mut random);
        assert_eq!(effect.heads[0].phase, 3);
        assert_eq!(effect.heads[0].sprite.color >> 24, 0);
        let rotation = effect.heads[0].sprite.rotation;
        effect.update(0, &mut random);
        let head = &effect.heads[0];
        assert_eq!((head.age, head.phase), (0, 0));
        assert_eq!(head.sprite.rotation[..2], rotation[..2]);
        effect.write_mesh(&mut host);
        assert!(
            host.quads
                .iter()
                .flatten()
                .all(|v| { v.color >> 24 == 0 && v.position.iter().all(|n| n.is_finite()) })
        );
    }
}
