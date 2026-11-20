use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

const OPAQUE: u8 = 128;

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

struct Head {
    sprite: Sprite,
    start: [f32; 2],
    end: [f32; 2],
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
        match self.phase {
            Phase::FadeIn => {
                self.sprite.alpha = lerp(0., OPAQUE as f32, fade_in, age) as u8;
                if age > fade_in {
                    self.enter(Phase::Roll);
                }
            }
            Phase::Roll => {
                for axis in 0..2 {
                    self.sprite.position[axis] =
                        lerp(self.start[axis], self.end[axis], travel, age);
                }
                self.sprite.angle += self.spin;
                if age > travel {
                    self.enter(Phase::FadeOut);
                }
            }
            Phase::FadeOut => {
                self.sprite.angle += self.spin;
                self.sprite.alpha = lerp(OPAQUE as f32, 0., fade_out, age) as u8;
                if age > fade_out {
                    self.sprite.alpha = 0;
                    self.enter(Phase::Inactive);
                }
            }
            Phase::Inactive => {}
        }

        let lifetime = self.trails.len() as u32;
        if self.phase == Phase::Roll {
            if let Some(trail) = self.trails.iter_mut().find(|trail| trail.flight.is_none()) {
                let mut point = self.sprite.position;
                let mut points = vec![point];
                for (x_range, x_offset, y_range, y_offset) in [
                    (60., 30., 20., 0.),
                    (80., 40., 40., 20.),
                    (80., 40., 40., 20.),
                ] {
                    point[0] =
                        (random.next_float() as f64 * x_range + point[0] as f64 - x_offset) as f32;
                    point[1] =
                        (random.next_float() as f64 * y_range + point[1] as f64 - y_offset) as f32;
                    points.push(point);
                }
                trail.sprite = Sprite {
                    scale: self.sprite.scale * 0.4,
                    ..self.sprite
                };
                trail.flight = Some((Path::new_extended(points, lifetime as f32, 20.), 0));
                // Creating a trail also changes the head's spin for its next update.
                self.spin = spin(random);
            }
        }
        for trail in &mut self.trails {
            let Some((path, age)) = &mut trail.flight else {
                continue;
            };
            trail.sprite.scale *= 0.999;
            trail.sprite.position = path.at(*age as i32 as f32);
            trail.sprite.alpha = lerp(OPAQUE as f32, 0., lifetime as f32, *age as i32 as f32) as u8;
            *age = age.wrapping_add(delta as u32);
            if *age >= lifetime {
                trail.flight = None;
                trail.sprite.alpha = 0;
            }
        }
        self.age = self.age.wrapping_add(delta);
    }
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
        let p = host.parameters.map(f32::from_bits);
        let group = (p[0] as i32 as u32).max(1) as usize;
        let count = host.quads.len() / group;
        let pivot = image.map(|axis| axis * 0.5);
        let heads = (0..count)
            .map(|i| {
                let shift = |axis: usize| p[9 + axis] as f64 / count as f64 * i as f64;
                let start = std::array::from_fn(|i| (p[1 + i] as f64 + shift(i)) as f32);
                let end = std::array::from_fn(|i| (p[3 + i] as f64 + shift(i)) as f32);
                let position = [start[0], start[1], 0.];
                let sprite = Sprite {
                    position,
                    pivot,
                    angle: 0.,
                    scale: (p[5] as f64 / 100.) as f32,
                    alpha: OPAQUE,
                };
                Head {
                    sprite,
                    start,
                    end,
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
            durations: [p[6], p[7], p[8]],
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
}
