use super::{Transform, particle_curve::Path};
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

fn fade(from: f32, to: f32, start: f32, end: f32, time: f32) -> f32 {
    if time <= start {
        from
    } else if time >= end {
        to
    } else {
        ((time - start) / (end - start)) * (to - from) + from
    }
}

#[derive(Default)]
struct Sprite {
    position: [f32; 3],
    origin: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    alpha: u32,
    region: u32,
}

impl Sprite {
    fn initial() -> Self {
        Self {
            scale: [1.; 3],
            alpha: 255,
            ..Self::default()
        }
    }
}

struct Trail {
    age: i32,
    active: bool,
    sprite: Sprite,
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    previous: [f32; 3],
    direction: [f32; 3],
    sprite: Sprite,
    trails: Vec<Trail>,
}

pub(super) struct Sakura {
    scale: f32,
    distance: f32,
    duration: f32,
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Sakura {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = integer(p[0]) as u32;
        let count = if group == 0 {
            host.quads.len()
        } else {
            host.quads.len() / group as usize
        };
        let mut out = Self {
            scale: p[1] / 100.,
            distance: p[2],
            duration: p[3],
            image,
            visible: if integer(p[4]) != 0 {
                count as u32 / 2
            } else {
                1
            },
            particles: Vec::with_capacity(count),
        };
        for i in 0..count {
            let mut particle = Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                path: Path::new(vec![[0.; 3]; 2], 0., 0.),
                previous: [0.; 3],
                direction: [0.; 3],
                sprite: Sprite::initial(),
                trails: (0..group.saturating_sub(1))
                    .map(|_| Trail {
                        age: 0,
                        active: false,
                        sprite: Sprite::initial(),
                    })
                    .collect(),
            };
            Self::spawn(
                &mut particle,
                out.image,
                out.scale,
                out.distance,
                out.duration,
                random,
            );
            if integer(p[4]) != 0 && i < out.visible as usize {
                particle.age = integer((random.next_float() as f64 * out.duration as f64) as f32);
                if particle.age as f32 > out.duration * 0.9 {
                    particle.phase = 1;
                    particle.phase_time = particle.age;
                }
            }
            out.particles.push(particle);
        }
        out
    }

    fn spawn(
        p: &mut Particle,
        image: [f32; 2],
        scale: f32,
        max_distance: f32,
        duration: f32,
        random: &mut ScriptRandom,
    ) {
        let s = &mut p.sprite;
        s.origin[..2].copy_from_slice(&image.map(|n| n * 0.5));
        s.region = random.next_integer() & 1;
        s.scale[0] = (random.next_float() * scale) * 0.6 + scale * 0.4;
        s.scale[1] = s.scale[0] * 1.3;
        s.alpha = 0;
        s.position = [(random.next_integer() % 1280) as f32 + 480., -50., 0.];
        s.rotation = std::array::from_fn(|_| {
            (random.next_float() as f64 * std::f32::consts::PI as f64) as f32
        });
        p.age = 0;
        p.phase = 0;
        p.previous = s.position;
        let mut points = vec![s.position];
        let initial = normalized([-random.next_float(), random.next_float(), 0.]);
        let distance = (random.next_float() * max_distance) * 0.2 + max_distance * 0.8;
        let mut previous = std::array::from_fn(|i| s.position[i] + initial[i] * distance);
        points.push(previous);
        p.direction = normalized(std::array::from_fn(|i| previous[i] - s.position[i]));
        let radians = (-30f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let sine = (radians as f64).sin() as f32;
        let cosine = (radians as f64).cos() as f32;
        let negative = [
            ((cosine * p.direction[0]) as f64 - sine as f64 * p.direction[1] as f64) as f32,
            ((cosine * p.direction[1]) as f64 + sine as f64 * p.direction[0] as f64) as f32,
        ];
        let positive = [
            ((cosine * p.direction[0]) as f64 + sine as f64 * p.direction[1] as f64) as f32,
            ((cosine * p.direction[1]) as f64 - sine as f64 * p.direction[0] as f64) as f32,
        ];
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let delta = positive[i] - negative[i];
            ((delta as f64).powf(2.) as f32 as f64).sqrt() as f32
        });
        for _ in 0..12 {
            let delta: [f32; 2] = std::array::from_fn(|i| {
                (random.next_float() as f64 * spread[i] as f64 - (spread[i] * 0.5) as f64) as f32
            });
            let next = normalized([p.direction[0] + delta[0], p.direction[1] + delta[1], 0.]);
            let distance = (random.next_float() * max_distance) * 0.2 + max_distance * 0.8;
            previous = std::array::from_fn(|i| previous[i] + next[i] * distance);
            points.push(previous);
            if random.next_integer().is_multiple_of(100) {
                break;
            }
        }
        p.path = Path::new(points, duration, duration / 5.);
        for trail in &mut p.trails {
            trail.age = 0;
            trail.active = false;
            trail.sprite.region = s.region;
            trail.sprite.position = s.position;
            trail.sprite.alpha = 0;
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(10) {
            self.visible = self.visible.wrapping_add(10);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for p in self.particles.iter_mut().take(self.visible as usize) {
            if p.phase <= 1 {
                let position = p.path.at(p.age as f32);
                p.sprite.position[..2].copy_from_slice(&position[..2]);
                p.sprite.alpha = if p.phase == 0 {
                    255
                } else {
                    integer(fade(
                        255.,
                        0.,
                        p.phase_time as f32,
                        self.duration,
                        p.age as f32,
                    )) as u32
                        & 255
                };
                for n in &mut p.sprite.rotation {
                    *n = (random.next_float() as f64 / 20. + *n as f64) as f32;
                }
                if p.phase == 0 {
                    if p.age > integer((self.duration * 0.9 + 0.5).floor()) {
                        p.phase = 1;
                        p.phase_time = p.age;
                    }
                } else if p.age as f32 > self.duration {
                    p.phase = 2;
                    p.phase_time = p.age;
                }
            } else {
                Self::spawn(
                    p,
                    self.image,
                    self.scale,
                    self.distance,
                    self.duration,
                    random,
                );
            }
            if p.phase == 0 {
                if let Some((j, t)) = p.trails.iter_mut().enumerate().find(|(_, t)| !t.active) {
                    let motion = std::array::from_fn(|k| p.sprite.position[k] - p.previous[k]);
                    if length(motion) != 0. {
                        t.sprite.rotation = p.sprite.rotation;
                        t.sprite.position = [
                            (p.previous[0] + p.sprite.position[0]) * 0.5,
                            (p.previous[1] + p.sprite.position[1]) * 0.5,
                            j as f32 + 1.,
                        ];
                    } else {
                        t.sprite.position = p.sprite.position;
                    }
                    t.sprite.origin = p.sprite.origin;
                    t.sprite.scale = p.sprite.scale;
                    t.active = true;
                }
            }
            let count = p.trails.len();
            for t in &mut p.trails {
                if t.active {
                    t.sprite.scale = t.sprite.scale.map(|n| n * 0.8);
                    t.sprite.alpha =
                        integer(fade(90., 0., 0., count as f32, t.age as f32)) as u32 & 255;
                    t.age = t.age.wrapping_add(1);
                    if t.age as u32 >= count as u32 {
                        t.age = 0;
                        t.active = false;
                    }
                }
            }
            p.previous = p.sprite.position;
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let count = self.particles.len();
        for (i, p) in self.particles.iter().enumerate() {
            let start = count + i * p.trails.len();
            let sprites = std::iter::once((i, &p.sprite)).chain(
                p.trails
                    .iter()
                    .enumerate()
                    .map(|(j, t)| (start + j, &t.sprite)),
            );
            for (index, s) in sprites {
                let mut transform = Transform::default();
                transform.set_scale(s.scale);
                transform.set_rotation(s.rotation);
                let mut matrix = *transform.matrix();
                matrix[12..15].copy_from_slice(&s.position);
                for (j, v) in host.quads[index].iter_mut().enumerate() {
                    let point = transform_vector(
                        [
                            (j & 1) as f32 * self.image[0] - s.origin[0],
                            (j >> 1) as f32 * self.image[1] - s.origin[1],
                            -s.origin[2],
                            1.,
                        ],
                        matrix,
                    );
                    v.position.copy_from_slice(&point[..3]);
                    v.rhw = point[3];
                    v.color = 0xffffff | s.alpha << 24;
                    v.uv = [
                        (s.region as f32 + (j & 1) as f32) * 0.5,
                        (j >> 1) as f32 * 0.5,
                    ];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(count: usize, parameters: [f32; 5]) -> ParticleHost {
        let mut host = ParticleHost::default();
        host.reset_quads(count, [64., 128.]).unwrap();
        for (i, n) in parameters.into_iter().enumerate() {
            host.parameters[i] = n.to_bits();
        }
        host
    }

    #[test]
    fn petals_keep_curved_motion_grouped_trails_and_unused_quads() {
        let mut host = host(8, [3., 20., 120., 500., 1.]);
        let unused = host.quads[6..].to_vec();
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Sakura::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.visible, 1);
        assert_eq!(effect.particles.len(), 2);
        let head = &effect.particles[0];
        assert_eq!(head.age, 11);
        assert_eq!(head.path.points.len(), 14);
        assert_eq!(
            head.path.points[13].map(f32::to_bits),
            [0x444e_4493, 0x44a5_e945, 0]
        );
        assert_eq!(
            head.sprite.scale.map(f32::to_bits),
            [0x3e1a_1e18, 0x3e48_5a52, 0x3f80_0000]
        );
        effect.write_mesh(&mut host);
        assert!(host.quads[..6].iter().flatten().all(|v| v.color >> 24 == 0));
        for step in 1u32..=40 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, &mut random);
        }
        let head = &effect.particles[0];
        assert_eq!(head.age, 70);
        assert_eq!(
            head.sprite.position.map(f32::to_bits),
            [0x445e_df2d, 0x430c_2be6, 0]
        );
        assert_eq!(effect.visible, 2);
        assert_eq!(
            head.trails
                .iter()
                .map(|t| (t.age, t.active))
                .collect::<Vec<_>>(),
            [(0, false), (1, true)]
        );
        assert_eq!(head.trails[1].sprite.position[2], 2.);
        assert_eq!(head.trails[1].sprite.alpha, 90);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[6..], unused);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.], [0.5, 0.5]]
        );
    }

    #[test]
    fn fade_rounds_the_phase_boundary_and_respawn_preserves_phase_time() {
        let input = host(1, [0., 0., 60., 75., 0.]);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Sakura::new(&input, [64., 128.], &mut random);
        for step in 1u32..=206 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, &mut random);
            let expected = match step {
                45 => [69, 0, 0, 255],
                46 => [70, 69, 1, 255],
                47 => [71, 69, 1, 212],
                53 => [76, 69, 1, 0],
                205 => [69, 76, 0, 255],
                206 => [70, 69, 1, 255],
                _ => continue,
            };
            let p = &effect.particles[0];
            assert_eq!(
                [p.age as u32, p.phase_time as u32, p.phase, p.sprite.alpha],
                expected
            );
        }
        let mut empty = host(0, [6., -20., 0., 0., 1.]);
        let mut effect = Sakura::new(&empty, [64., 128.], &mut random);
        effect.update(0, &mut random);
        effect.write_mesh(&mut empty);
        assert!(empty.quads.is_empty());
    }
}
