use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

const HEAD_ALPHA: u8 = 128;

#[derive(Clone, Copy)]
struct Sprite {
    position: [f32; 3],
    pivot: [f32; 2],
    angle: f32,
    scale: f32,
    alpha: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    FadeIn,
    Roll,
    FadeOut,
    Inactive,
}

struct Trail {
    sprite: Sprite,
    flight: Option<(Path, u32)>,
}

enum Motion {
    Line { start: [f32; 2], end: [f32; 2] },
    Curve(Path),
}

struct Head {
    sprite: Sprite,
    motion: Motion,
    spin: f32,
    age: i32,
    phase_start: i32,
    phase: Phase,
    trails: Vec<Trail>,
}

impl Head {
    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_start = self.age;
    }

    fn update(&mut self, durations: [f32; 3], delta: i32, random: &mut ScriptRandom) {
        let [fade_in, travel, fade_out] = durations;
        let age = self.age.wrapping_sub(self.phase_start) as f32;
        let previous = self.sprite.position;
        match self.phase {
            Phase::FadeIn => {
                self.sprite.alpha = lerp(0., HEAD_ALPHA as f32, fade_in, age) as u8;
                if age > fade_in {
                    self.enter(Phase::Roll);
                }
            }
            Phase::Roll => {
                match &self.motion {
                    Motion::Line { start, end } => {
                        for axis in 0..2 {
                            self.sprite.position[axis] = lerp(start[axis], end[axis], travel, age);
                        }
                    }
                    Motion::Curve(path) => self.sprite.position = path.at(age),
                }
                self.sprite.angle += self.spin;
                if age > travel {
                    self.enter(Phase::FadeOut);
                }
            }
            Phase::FadeOut => {
                self.sprite.angle += self.spin;
                self.sprite.alpha = lerp(HEAD_ALPHA as f32, 0., fade_out, age) as u8;
                if age > fade_out {
                    self.sprite.alpha = 0;
                    self.enter(Phase::Inactive);
                }
            }
            Phase::Inactive => {}
        }

        let lifetime = self.trails.len() as u32;
        let origin = self.sprite.position;
        if self.phase == Phase::Roll {
            if let Some(trail) = self.trails.iter_mut().find(|trail| trail.flight.is_none()) {
                let path = match &self.motion {
                    Motion::Line { .. } => linear_trail(origin, lifetime, random),
                    Motion::Curve(head) => curved_trail(head, origin, previous, lifetime, random),
                };
                trail.sprite = Sprite {
                    scale: self.sprite.scale * 0.4,
                    ..self.sprite
                };
                trail.flight = Some((path, 0));
                // Creating a trail also changes the head's spin for its next update.
                self.spin = spin(random);
            }
        }
        let trail_alpha = match self.motion {
            Motion::Line { .. } => HEAD_ALPHA as f32,
            Motion::Curve(_) => 90.,
        };
        for trail in &mut self.trails {
            let Some((path, age)) = &mut trail.flight else {
                continue;
            };
            trail.sprite.scale *= 0.999;
            trail.sprite.position = path.at(*age as i32 as f32);
            trail.sprite.alpha = lerp(trail_alpha, 0., lifetime as f32, *age as i32 as f32) as u8;
            *age = age.wrapping_add(delta as u32);
            if *age >= lifetime {
                trail.flight = None;
                trail.sprite.alpha = 0;
            }
        }
        self.age = self.age.wrapping_add(delta);
    }
}

fn linear_trail(origin: [f32; 3], lifetime: u32, random: &mut ScriptRandom) -> Path {
    let mut point = origin;
    let mut points = vec![point];
    for (x_range, x_offset, y_range, y_offset) in [
        (60., 30., 20., 0.),
        (80., 40., 40., 20.),
        (80., 40., 40., 20.),
    ] {
        point[0] = (random.next_float() as f64 * x_range + point[0] as f64 - x_offset) as f32;
        point[1] = (random.next_float() as f64 * y_range + point[1] as f64 - y_offset) as f32;
        points.push(point);
    }
    Path::new_extended(points, lifetime as f32, 20.)
}

fn length(vector: [f32; 3]) -> f32 {
    let [x, y, z] = vector.map(f64::from);
    ((x * x + y * y + z * z) as f32).sqrt()
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length = length(vector);
    if length > 0. {
        vector.map(|n| n / length)
    } else {
        vector
    }
}

fn curved_trail(
    head: &Path,
    origin: [f32; 3],
    previous: [f32; 3],
    lifetime: u32,
    random: &mut ScriptRandom,
) -> Path {
    let mut direction = std::array::from_fn(|i| previous[i] - origin[i]);
    if length(direction) < 0.001 {
        let a = head.at(0.);
        let b = head.at(1.);
        direction = std::array::from_fn(|i| b[i] - a[i]);
    }
    let [x, y, z] = normalize(direction);
    let mut points = vec![origin];
    for (degrees, distance) in [(50., 10.), (80., 20.), (90., 20.)] {
        let angle = (degrees * std::f32::consts::PI as f64 / 180.) as f32;
        let time = (random.next_float() as f64 * 100.) as f32;
        let angle = lerp(-angle, angle, 100., time);
        let angle = (((angle as f64 * 1000.) as i32) as f64 / 1000.) as f32;
        let (sin, cos) = (angle as f64).sin_cos();
        let (sin, cos) = (sin as f32 as f64, cos as f32 as f64);
        let rotated = normalize([
            (cos * x as f64 - sin * y as f64) as f32,
            (sin * x as f64 + cos * y as f64) as f32,
            z,
        ]);
        let distance = (random.next_float() as f64 * distance + distance) as f32;
        points.push(std::array::from_fn(|i| origin[i] + rotated[i] * distance));
    }
    Path::new_extended_relative(points, lifetime as f32, 0.1)
}

fn lerp(from: f32, to: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    }
}

fn spin(random: &mut ScriptRandom) -> f32 {
    (random.next_float() as f64 * std::f32::consts::PI as f64 - std::f32::consts::FRAC_PI_2 as f64)
        as f32
}

pub(super) struct Rolling {
    heads: Vec<Head>,
    durations: [f32; 3],
    image: [f32; 2],
}

impl Rolling {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        Self::new_with_curve(host, image, random, false)
    }

    pub(super) fn new_curved(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Self {
        Self::new_with_curve(host, image, random, true)
    }

    fn new_with_curve(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        curved: bool,
    ) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = (p[0] as i32 as u32).max(1) as usize;
        let count = host.quads.len() / group;
        let pivot = image.map(|axis| axis * 0.5);
        let rails: Option<[[[f32; 3]; 3]; 2]> = curved.then(|| {
            std::array::from_fn(|i| {
                let points = (0..3)
                    .map(|j| [p[1 + 6 * i + 2 * j], p[2 + 6 * i + 2 * j], 0.])
                    .collect();
                let path = Path::new_extended_relative(points, 100., 1.5);
                [0., 50., 100.].map(|time| path.at(time))
            })
        });
        let scale = (p[if curved { 13 } else { 5 }] as f64 / 100.) as f32;
        let durations = if curved {
            [p[14], p[15], p[16]]
        } else {
            [p[6], p[7], p[8]]
        };
        let heads = (0..count)
            .map(|i| {
                let (position, motion) = if let Some([left, right]) = &rails {
                    let points: Vec<_> = left
                        .iter()
                        .zip(right)
                        .map(|(a, b)| {
                            std::array::from_fn(|axis| {
                                (a[axis] as f64
                                    + (a[axis] as f64 - b[axis] as f64) / count as f64 * i as f64)
                                    as f32
                            })
                        })
                        .collect();
                    (
                        points[0],
                        Motion::Curve(Path::new_extended_relative(points, durations[1], 0.5)),
                    )
                } else {
                    let shift = |axis: usize| p[9 + axis] as f64 / count as f64 * i as f64;
                    let start = std::array::from_fn(|i| (p[1 + i] as f64 + shift(i)) as f32);
                    let end = std::array::from_fn(|i| (p[3 + i] as f64 + shift(i)) as f32);
                    ([start[0], start[1], 0.], Motion::Line { start, end })
                };
                let sprite = Sprite {
                    position,
                    pivot,
                    angle: 0.,
                    scale,
                    alpha: HEAD_ALPHA,
                };
                Head {
                    sprite,
                    motion,
                    spin: spin(random),
                    age: 0,
                    phase_start: 0,
                    phase: Phase::FadeIn,
                    trails: (1..group)
                        .map(|_| Trail {
                            sprite: Sprite {
                                scale: 1.,
                                alpha: 0,
                                ..sprite
                            },
                            flight: None,
                        })
                        .collect(),
                }
            })
            .collect();
        Self {
            heads,
            durations,
            image,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let delta = delta as f32 as i64 as i32;
        for head in &mut self.heads {
            head.update(self.durations, delta, random);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let sprites = self
            .heads
            .iter()
            .map(|head| (&head.sprite, [0.5, 0.]))
            .chain(
                self.heads
                    .iter()
                    .flat_map(|head| head.trails.iter().map(|trail| (&trail.sprite, [0.5, 0.5]))),
            );
        for ((sprite, uv), quad) in sprites.zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([sprite.scale, sprite.scale, 1.]);
            transform.set_rotation([0., 0., sprite.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&sprite.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let [x, y] = [(i & 1) as f32, (i >> 1) as f32];
                let point = transform_vector(
                    [
                        x * self.image[0] - sprite.pivot[0],
                        y * self.image[1] - sprite.pivot[1],
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = (vertex.color & 0xff_ffff) | (u32::from(sprite.alpha) << 24);
                vertex.uv = [uv[0] + x * 0.5, uv[1] + y * 0.5];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_delta_rolls_and_emits_a_trail_without_advancing_its_lifetime() {
        let image = [32., 24.];
        let mut host = ParticleHost::default();
        host.reset_quads(3, image).unwrap();
        let unused = host.quads[2];
        let params = [2f32, 80., 160., 450., 260., 64., 3., 12., 7., 90., -20.];
        host.parameters[..params.len()].copy_from_slice(&params.map(f32::to_bits));
        let mut random = ScriptRandom::seeded(1);
        let mut effect = Rolling::new(&host, image, &mut random);
        effect.update(3, &mut random);
        effect.update(1, &mut random);
        assert!(effect.heads[0].phase == Phase::FadeIn);
        effect.update(0, &mut random);
        effect.write_mesh(&mut host);
        assert!(effect.heads[0].phase == Phase::Roll);
        assert_eq!(effect.heads[0].age, 4);
        assert_eq!(host.quads[0][0].color >> 24, 128);
        assert_eq!(host.quads[1][0].color >> 24, 128);
        assert_eq!(host.quads[1][0].uv, [0.5, 0.5]);

        let spin = effect.heads[0].spin;
        effect.update(0, &mut random);
        assert_eq!(effect.heads[0].sprite.angle, spin);
        assert_eq!(effect.heads[0].trails[0].flight.as_ref().unwrap().1, 0);
        effect.update(1, &mut random);
        effect.write_mesh(&mut host);
        assert!(effect.heads[0].trails[0].flight.is_none());
        assert_eq!(host.quads[1][0].color >> 24, 0);
        assert_eq!(host.quads[2], unused);
    }

    #[test]
    fn curved_rolling_extrapolates_the_formation_and_emits_a_fainter_trail() {
        let image = [32., 24.];
        let mut host = ParticleHost::default();
        host.reset_quads(8, image).unwrap();
        let params = [
            2f32, 80., 160., 220., 320., 480., -30., 100., 140., 310., 470., 610., 80., 52., 3.,
            12., 7.,
        ];
        host.parameters[..params.len()].copy_from_slice(&params.map(f32::to_bits));
        let mut random = ScriptRandom::seeded(1);
        let mut effect = Rolling::new_curved(&host, image, &mut random);
        assert_eq!(effect.heads[1].sprite.position, [75., 165., 0.]);
        effect.update(4, &mut random);
        effect.update(0, &mut random);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color >> 24, 128);
        assert_eq!(host.quads[4][0].color >> 24, 90);
        let before = effect.heads[0].sprite.position;
        effect.update(2, &mut random);
        effect.update(0, &mut random);
        let [x, y, _] = effect.heads[0].sprite.position;
        assert_eq!(before, [80., 160., 0.]);
        assert!((x - 175.5478).abs() < 0.001 && (y - 203.4536).abs() < 0.001);
    }
}
