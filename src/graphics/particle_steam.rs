use super::particle_curve::Path;
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

fn fade(from: f32, to: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    }
}

fn rotate(vector: [f32; 3], angle: f32) -> [f32; 3] {
    let reduced = (integer(angle * 1000.) as f32 / 1000.) as f64;
    let cosine = reduced.cos() as f32;
    let sine = reduced.sin() as f32;
    let matrix = [
        cosine, sine, 0., 0., -sine, cosine, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    // 路径方向保留标量乘加顺序，与绘图顶点的向量归约分开。
    std::array::from_fn(|i| {
        ((matrix[i] * vector[0] + matrix[i + 4] * vector[1]) + matrix[i + 8] * vector[2])
            + matrix[i + 12]
    })
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    position: [f32; 3],
    scale: f32,
    alpha: u32,
    alpha_max: f32,
    region: usize,
}

pub(super) struct Steam {
    speed: [f32; 2],
    heading: [f32; 2],
    scale: [f32; 2],
    alpha: [f32; 2],
    lives: [i32; 3],
    rate: u32,
    batch: u32,
    base: Path,
    axis: [f32; 3],
    image: [f32; 2],
    visible: u32,
    warm: i32,
    particles: Vec<Particle>,
}

impl Steam {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let lower = |v: f32, n: f32| if v < n { n } else { v };
        let clamp = |v: f32| v.clamp(-std::f32::consts::PI, std::f32::consts::PI);
        let from = [p[11], p[12], 0.];
        let to = [p[13], p[14], 0.];
        let delta = std::array::from_fn(|i| from[i] - to[i]);
        let mut axis = rotate(delta, std::f32::consts::FRAC_PI_2);
        let n = length(axis);
        if n > 0. {
            axis = axis.map(|v| v / n);
        }
        let mut out = Self {
            speed: [lower(p[0], 10.), lower(p[1], 0.)],
            heading: [clamp(p[2] / 100.), clamp(p[3] / 100.)],
            scale: [lower(p[4] / 100., 0.01), lower(p[5] / 100., 0.)],
            alpha: [lower(p[6], 0.), lower(p[7], 0.)],
            lives: std::array::from_fn(|i| integer(p[i + 8]).max(0)),
            rate: 61i32.wrapping_sub(integer(p[15])).clamp(1, 60) as u32,
            batch: integer(p[16]).max(1) as u32,
            base: Path::new(vec![from, to], 100., 1.),
            axis,
            image,
            visible: if integer(p[17]) != 0 {
                host.quads.len() as u32 / 2
            } else {
                1
            },
            warm: integer(p[17]),
            particles: Vec::with_capacity(host.quads.len()),
        };
        for index in 0..host.quads.len() {
            let mut particle = out.spawn(random);
            if out.warm != 0 && index < out.visible as usize {
                let total = out.lives[2]
                    .wrapping_add(out.lives[1])
                    .wrapping_add(out.lives[0]);
                particle.age = integer(random.next_float() * total as f32);
                if particle.age > out.lives[0] {
                    particle.phase += 1;
                    particle.phase_time = particle.phase_time.wrapping_add(out.lives[0]);
                    particle.alpha = 128;
                }
                if particle.age > out.lives[1] {
                    particle.phase += 1;
                    particle.phase_time = particle.phase_time.wrapping_add(out.lives[1]);
                }
            }
            out.particles.push(particle);
        }
        out
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let region = (random.next_integer() & 3) as usize;
        let scale = (random.next_float() * self.scale[1] - self.scale[1] * 0.5) + self.scale[0];
        let start = self.base.at(random.next_float() * 100.);
        let mut points = vec![start];
        let angle =
            (random.next_float() * self.heading[1] - self.heading[1] * 0.5) + self.heading[0];
        let distance = (random.next_float() * self.speed[1] + self.speed[1] * 0.5) + self.speed[0];
        let direction = rotate(self.axis, angle);
        let mut current = std::array::from_fn(|j| direction[j] * distance + start[j]);
        points.push(current);
        for _ in 0..3 {
            if random.next_integer().is_multiple_of(5) {
                continue;
            }
            let angle =
                (random.next_float() * self.heading[1] - self.heading[1] * 0.5) + self.heading[0];
            let distance =
                (random.next_float() * self.speed[1] + self.speed[1] * 0.5) + self.speed[0];
            let direction = rotate(self.axis, angle);
            current = std::array::from_fn(|j| current[j] + direction[j] * distance);
            points.push(current);
        }
        let duration = self.lives[2]
            .wrapping_add(self.lives[1])
            .wrapping_add(self.lives[0]) as f32;
        let alpha_max = (random.next_float() * self.alpha[1] - self.alpha[1] * 0.5) + self.alpha[0];
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            path: Path::new(points, duration, duration * 0.4),
            position: start,
            scale,
            alpha: 0,
            alpha_max,
            region,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.rate) {
            self.visible = self.visible.wrapping_add(self.batch);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        let elapsed = integer(delta as f32);
        for index in 0..self.visible as usize {
            if self.particles[index].phase > 2 {
                let phase_time = self.particles[index].phase_time;
                self.particles[index] = self.spawn(random);
                self.particles[index].phase_time = phase_time;
            } else {
                let p = &mut self.particles[index];
                let time = p.age.wrapping_sub(p.phase_time);
                p.alpha = match p.phase {
                    0 => {
                        integer(fade(0., p.alpha_max, self.lives[0] as f32, p.age as f32)) as u32
                            & 255
                    }
                    1 => integer(p.alpha_max) as u32 & 255,
                    _ => {
                        integer(fade(p.alpha_max, 0., self.lives[2] as f32, time as f32)) as u32
                            & 255
                    }
                };
                p.position = p.path.at(p.age as f32);
                let complete = match p.phase {
                    0 => p.age >= self.lives[0],
                    1 => time >= self.lives[1],
                    _ => time > self.lives[2],
                };
                if complete {
                    p.phase += 1;
                    p.phase_time = p.age;
                }
            }
            self.particles[index].age = self.particles[index].age.wrapping_add(elapsed);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
            matrix[0] = p.scale;
            matrix[5] = p.scale;
            matrix[12..15].copy_from_slice(&p.position);
            let region = [[0., 0.], [0.5, 0.], [0., 0.5], [0.5, 0.5]][p.region];
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
                v.color = 0xffffff | (p.alpha << 24);
                v.uv = [
                    region[0] + (i & 1) as f32 * 0.5,
                    region[1] + (i >> 1) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefilled_particles_fade_and_respawn_with_shared_randomness() {
        let mut host = ParticleHost::default();
        host.reset_quads(3, [64., 128.]).unwrap();
        for (i, value) in [
            120f32, 80., 80., 90., 100., 70., 60., 50., 40., 10., 60., 0., 720., 1250., 720., 55.,
            1., 1.,
        ]
        .into_iter()
        .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Steam::new(&host, [64., 128.], &mut random);
        let mut peek = random.clone();
        assert_eq!(peek.next_integer(), 3_186_491_980);
        assert_eq!(peek.next_float().to_bits(), 1_059_552_144);
        assert_eq!(effect.visible, 1);
        let p = &effect.particles[0];
        assert_eq!([p.age, p.phase_time, p.phase as i32], [61, 50, 2]);
        assert_eq!(p.alpha, 128);
        assert_eq!(
            p.position.map(f32::to_bits),
            [1_117_320_186, 1_144_258_560, 0]
        );
        for (delta, expected) in [
            (0, [61, 50, 2, 45]),
            (40, [101, 50, 2, 45]),
            (0, [101, 50, 2, 8]),
            (70, [171, 50, 2, 8]),
            (0, [171, 171, 3, 0]),
            (0, [0, 171, 0, 0]),
            (1, [1, 171, 0, 0]),
        ] {
            effect.update(delta, &mut random);
            let p = &effect.particles[0];
            assert_eq!(
                [p.age as u32, p.phase_time as u32, p.phase, p.alpha],
                expected
            );
        }
        assert_eq!(
            effect.particles[0].position.map(f32::to_bits),
            [1_142_167_893, 1_144_258_560, 0]
        );
        assert_eq!(random.next_integer(), 294_199_790);
        assert_eq!(random.next_float().to_bits(), 1_062_843_635);
    }

    #[test]
    fn fade_completion_is_strict_and_respawn_preserves_phase_time() {
        let mut host = ParticleHost::default();
        host.reset_quads(1, [1., 1.]).unwrap();
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Steam::new(&host, [1., 1.], &mut random);
        assert_eq!(effect.axis, [0.; 3]);
        for (delta, phase, phase_time, age) in [
            (0, 1, 0, 0),
            (0, 2, 0, 0),
            (1, 2, 0, 1),
            (0, 3, 1, 1),
            (0, 0, 1, 0),
        ] {
            effect.update(delta, &mut random);
            let p = &effect.particles[0];
            assert_eq!((p.phase, p.phase_time, p.age), (phase, phase_time, age));
            assert_eq!(p.alpha, 0);
            assert!(p.position.iter().all(|v| v.is_finite()));
        }
    }
}
