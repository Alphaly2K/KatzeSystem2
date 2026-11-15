use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn length(vector: [f32; 3]) -> f32 {
    let squares = vector.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
}

fn normalized(vector: [f32; 3]) -> [f32; 3] {
    let size = length(vector);
    if size > 0. {
        vector.map(|n| n / size)
    } else {
        vector
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

struct Path {
    points: [[f32; 3]; 4],
    tangents: [[f32; 3]; 4],
    lengths: [f32; 4],
    total: f32,
    duration: f32,
}

impl Path {
    fn new(points: [[f32; 3]; 4], duration: f32, strength: f32) -> Self {
        let lengths = std::array::from_fn(|i| {
            if i == 0 {
                0.
            } else {
                length(std::array::from_fn(|j| points[i][j] - points[i - 1][j]))
            }
        });
        let total = lengths.iter().fold(0f32, |a, b| a + b);
        let tangents = if duration == 0. {
            [[0.; 3]; 4]
        } else {
            std::array::from_fn(|i| {
                let delta = match i {
                    0 => std::array::from_fn(|j| points[1][j] - points[0][j]),
                    3 => std::array::from_fn(|j| points[3][j] - points[2][j]),
                    _ => std::array::from_fn(|j| {
                        (points[i + 1][j] - points[i][j]) - (points[i - 1][j] - points[i][j])
                    }),
                };
                let size = length(delta);
                delta.map(|n| (n / size) * strength)
            })
        };
        Self {
            points,
            tangents,
            lengths,
            total,
            duration,
        }
    }

    fn at(&self, mut time: f32) -> [f32; 3] {
        if time <= 0. {
            return self.points[0];
        }
        if time >= self.duration {
            time = self.duration;
        }
        // 先按总长度换算距离，再选择区间，保留边界判断前的逐步舍入。
        let distance = (time / self.duration) * self.total;
        let mut previous = 0.;
        for i in 0..3 {
            let next = previous + self.lengths[i + 1];
            if next > distance {
                let t = (distance - previous) / self.lengths[i + 1];
                let square = t * t;
                let cube = square * t;
                let basis = [
                    [2., -2., 1., 1.],
                    [-3., 3., -2., -1.],
                    [0., 0., 1., 0.],
                    [1., 0., 0., 0.],
                ];
                let coefficients = basis.map(|row| {
                    std::array::from_fn::<_, 3, _>(|j| {
                        ((self.points[i][j] * row[0] + self.points[i + 1][j] * row[1])
                            + self.tangents[i][j] * row[2])
                            + self.tangents[i + 1][j] * row[3]
                    })
                });
                return std::array::from_fn(|j| {
                    ((coefficients[0][j] * cube + coefficients[1][j] * square)
                        + coefficients[2][j] * t)
                        + coefficients[3][j]
                });
            }
            previous = next;
        }
        self.points[3]
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    angle: f32,
    scale: f32,
    lives: [i32; 3],
    path: Path,
    position: [f32; 3],
    alpha: u32,
}

pub(super) struct Note {
    anchor: [f32; 3],
    mirror: i32,
    scale: f32,
    lives: [i32; 3],
    center: [f32; 2],
    factor: f32,
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Note {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let parameters = host.parameters.map(f32::from_bits);
        let group = integer(parameters[0]) as u32;
        let count = if group == 0 {
            host.quads.len()
        } else {
            host.quads.len() / group as usize
        };
        let mut scale = parameters[4] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let mut effect = Self {
            anchor: [parameters[1], parameters[2], 0.],
            mirror: integer(parameters[3]),
            scale,
            lives: [
                integer(parameters[5]),
                integer(parameters[6]),
                integer(parameters[7]),
            ],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            factor: if image[1] > 256. { 1. } else { 0.5 },
            image,
            visible: 1,
            particles: Vec::with_capacity(count),
        };
        for _ in 0..count {
            effect.particles.push(effect.spawn(random));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let scale =
            ((random.next_float() * self.scale) as f64 * 0.4 + (self.scale * 0.6) as f64) as f32;
        let lives = self
            .lives
            .map(|n| integer((random.next_float() * n as f32) * 0.5 + n as f32 * 0.5));
        let angle = (random.next_float() as f64 - 0.5) as f32;
        let distance = (self.scale * 80.) * self.factor;
        let initial = normalized([if self.mirror != 0 { 1. } else { -1. }, -1., 0.]);
        let first_distance =
            (random.next_float() as f64 * distance as f64 + distance as f64) as f32;
        let mut previous = std::array::from_fn(|i| self.anchor[i] + initial[i] * first_distance);
        let direction = normalized(std::array::from_fn(|i| previous[i] - self.anchor[i]));
        let radians = (-50f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let sine = (radians as f64).sin() as f32;
        let cosine = (radians as f64).cos() as f32;
        // 第一项先舍入到 f32，再与另一项作 double 加减。
        let negative = [
            ((cosine * direction[0]) as f64 - sine as f64 * direction[1] as f64) as f32,
            ((cosine * direction[1]) as f64 + sine as f64 * direction[0] as f64) as f32,
        ];
        let positive = [
            ((cosine * direction[0]) as f64 + sine as f64 * direction[1] as f64) as f32,
            ((cosine * direction[1]) as f64 - sine as f64 * direction[0] as f64) as f32,
        ];
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let delta = positive[i] - negative[i];
            ((delta as f64).powf(2.) as f32 as f64).sqrt() as f32
        });
        let points = std::array::from_fn(|_| {
            let delta: [f32; 2] = std::array::from_fn(|i| {
                (random.next_float() as f64 * spread[i] as f64 - (spread[i] * 0.5) as f64) as f32
            });
            let next = normalized([direction[0] + delta[0], direction[1] + delta[1], 0.]);
            let amount = (random.next_float() as f64 * distance as f64 + distance as f64) as f32;
            let point = std::array::from_fn(|i| previous[i] + next[i] * amount);
            previous = point;
            point
        });
        let path = Path::new(
            points,
            lives.into_iter().fold(0i32, |a, b| a.wrapping_add(b)) as f32,
            (self.scale * 120.) * self.factor,
        );
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            angle,
            scale,
            lives,
            path,
            position: self.anchor,
            alpha: 0,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(51) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for i in 0..self.visible as usize {
            if self.particles[i].phase > 2 {
                self.particles[i] = self.spawn(random);
            } else {
                let particle = &mut self.particles[i];
                particle.position = particle.path.at(particle.age as f32);
                let elapsed = particle.age.wrapping_sub(particle.phase_time);
                if particle.phase != 1 {
                    particle.alpha = integer(if particle.phase == 0 {
                        fade(0., 255., particle.lives[0] as f32, elapsed as f32)
                    } else {
                        fade(255., 0., particle.lives[2] as f32, elapsed as f32)
                    }) as u32
                        & 255;
                }
                if elapsed > particle.lives[particle.phase as usize] {
                    particle.phase = if particle.phase == 2 {
                        10
                    } else {
                        particle.phase + 1
                    };
                    particle.phase_time = particle.age;
                }
            }
            let particle = &mut self.particles[i];
            for (axis, value) in source.into_iter().enumerate() {
                particle.position[axis] += value - self.center[axis];
            }
            particle.age = particle.age.wrapping_add(integer(delta as f32));
        }
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
                        match i {
                            0 | 1 => -self.image[1] * 0.5,
                            // 第三个角点的下沿使用半宽。
                            2 => self.image[0] * 0.5,
                            _ => self.image[1] * 0.5,
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

    #[test]
    fn note_keeps_curve_rounding_activation_fade_boundaries_and_respawn_randomness() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(3, [96., 300.]).unwrap();
        for (i, value) in [1f32, 778., 214., 1., 54., 30., 60., 60.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Note::new(&host, [96., 300.], &mut random);
        assert_eq!(effect.particles.len(), 3);
        assert_eq!(
            effect.particles[0].path.points[0].map(f32::to_bits),
            [0x445b_b5a9, 0x4323_814a, 0]
        );
        for step in 1u32..=95 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, [0., 0.], &mut random);
            let expected = match step {
                7 => [6, 0, 0, 2, 90, 0x4376_0c20, 0xc34c_966a],
                17 => [21, 18, 1, 2, 255, 0x4381_042a, 0xc35a_8447],
                55 => [84, 69, 2, 3, 136, 0x43a8_2df4, 0xc387_c9e2],
                68 => [1, 0, 0, 3, 0, 0x430a_0000, 0xc312_0000],
                95 => [42, 30, 1, 3, 255, 0x439a_7df8, 0xc388_08bc],
                _ => continue,
            };
            let particle = &effect.particles[0];
            assert_eq!(
                [
                    particle.age as u32,
                    particle.phase_time as u32,
                    particle.phase,
                    effect.visible,
                    particle.alpha,
                    particle.position[0].to_bits(),
                    particle.position[1].to_bits(),
                ],
                expected
            );
        }
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..95 {
            expected.next_integer();
        }
        for _ in 0..90 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());

        host.reset_quads(1, [96., 300.]).unwrap();
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Note::new(&host, [96., 300.], &mut random);
        for step in 1u32..=68 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, [0., 0.], &mut random);
        }
        assert_eq!(effect.particles[0].path.points[3][1].to_bits(), 0xc1d6_2b88);
    }
}
