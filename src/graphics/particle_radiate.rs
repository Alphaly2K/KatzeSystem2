use super::{Transform, particle_curve::TwoPointPath};
use crate::kcs::{ParticleHost, ScriptRandom, native_float_math, transform_vector};
use std::ops::Range;

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

#[derive(Clone)]
struct Sprite {
    age: i32,
    phase_time: i32,
    phase: u32,
    position: [f32; 3],
    rotation: f32,
    scale: [f32; 3],
    pivot: [f32; 2],
    alpha: u8,
}

struct Head {
    index: usize,
    trails: Range<usize>,
    previous: [f32; 3],
    path: TwoPointPath,
}

pub(super) struct Radiate {
    anchor: [f32; 2],
    life: f32,
    scale: [f32; 2],
    distance: f32,
    visible: usize,
    image: [f32; 2],
    heads: Vec<Head>,
    sprites: Vec<Sprite>,
}

impl Radiate {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let width = integer(p[0]) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(width)
            .unwrap_or(host.quads.len());
        let mut effect = Self {
            anchor: [p[2], p[3]],
            life: p[1],
            scale: [p[4] / 1000., p[5] / 1000.],
            distance: p[6],
            visible: 0,
            image,
            heads: Vec::with_capacity(count),
            sprites: vec![
                Sprite {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    position: [0.; 3],
                    rotation: 0.,
                    scale: [1.; 3],
                    pivot: [0.; 2],
                    alpha: 255,
                };
                host.quads.len()
            ],
        };
        let trails = width.saturating_sub(1);
        for index in 0..count {
            let first = count + index * trails;
            let (position, path) = effect.spawn(random);
            effect.heads.push(Head {
                index,
                trails: first..first + trails,
                previous: position,
                path,
            });
            effect.reset_group(index, position);
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> ([f32; 3], TwoPointPath) {
        let start = [
            self.anchor[0] + (random.next_float() * 2. - 1.) * 100.,
            self.anchor[1] + (random.next_float() * 2. - 1.) * 100.,
            0.,
        ];
        let mut direction = [start[0] - self.anchor[0], start[1] - self.anchor[1], 0.];
        let size = length(direction);
        if size > 0. {
            direction = direction.map(|n| n / size);
        }
        let end = [
            self.anchor[0] + direction[0] * self.distance,
            self.anchor[1] + direction[1] * self.distance,
            0.,
        ];
        let duration = (random.next_float() as f64 * (self.life * 0.7) as f64
            + (self.life * 0.3) as f64) as f32;
        (
            start,
            TwoPointPath::new([start, end], duration, self.distance),
        )
    }

    fn reset_group(&mut self, index: usize, position: [f32; 3]) {
        let group = &mut self.heads[index];
        group.previous = position;
        let head = &mut self.sprites[group.index];
        head.age = 0;
        head.phase = 0;
        head.position = position;
        head.scale = [self.scale[0], self.scale[0], 1.];
        head.pivot = self.image.map(|n| n * 0.5);
        head.alpha = 0;
        for trail in &mut self.sprites[group.trails.clone()] {
            trail.age = 0;
            trail.phase = 0;
            trail.position = position;
            trail.alpha = 0;
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        self.visible = (self.visible + 1).min(self.heads.len());
        for index in 0..self.visible {
            let group = &self.heads[index];
            let head = &mut self.sprites[group.index];
            if head.phase <= 1 {
                head.position = group.path.at(head.age as f32);
                let duration = if head.phase == 0 {
                    let scale = fade(
                        self.scale[0],
                        self.scale[1],
                        group.path.duration,
                        head.age as f32,
                    );
                    head.scale[..2].fill(scale);
                    group.path.duration
                } else {
                    self.life
                };
                if head.age as f32 > duration {
                    head.phase = 2;
                    head.phase_time = head.age;
                }
            } else {
                let (position, path) = self.spawn(random);
                self.heads[index].path = path;
                self.reset_group(index, position);
            }
            let group = &mut self.heads[index];
            let head = self.sprites[group.index].clone();
            if head.phase == 0 {
                if let Some(trail) = self.sprites[group.trails.clone()]
                    .iter_mut()
                    .find(|p| p.phase == 0)
                {
                    let delta = std::array::from_fn(|i| head.position[i] - group.previous[i]);
                    let size = length(delta);
                    trail.pivot = head.pivot;
                    trail.scale = head.scale;
                    if size == 0. {
                        trail.position = head.position;
                    } else {
                        let direction = delta.map(|n| n / size);
                        let cosine = (direction[0] * 0. + direction[1]) + direction[2] * 0.;
                        let angle = f32::from_bits(native_float_math(10, &[cosine.to_bits()]));
                        let sign = (delta[1] * 0. + delta[0]) + delta[2] * 0.;
                        trail.rotation = if sign > 0. { -angle } else { angle };
                        trail.position =
                            std::array::from_fn(|i| (head.position[i] + group.previous[i]) * 0.5);
                        trail.scale[1] *= size * 0.7;
                    }
                    trail.phase = 1;
                }
            }
            for trail in &mut self.sprites[group.trails.clone()] {
                if trail.phase != 0 {
                    trail.alpha =
                        integer(fade(255., 0., group.trails.len() as f32, trail.age as f32)) as u8;
                    trail.age = trail.age.wrapping_add(integer(delta as f32));
                    if trail.age as u32 >= group.trails.len() as u32 {
                        trail.age = 0;
                        trail.phase = 0;
                        trail.alpha = 0;
                    }
                }
            }
            group.previous = head.position;
            let head = &mut self.sprites[group.index];
            head.age = head.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.sprites.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale(p.scale);
            transform.set_rotation([0., 0., p.rotation]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - p.pivot[0],
                        (i >> 1) as f32 * self.image[1] - p.pivot[1],
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | (u32::from(p.alpha) << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_trails_recycle_by_elapsed_time_and_respawn_preserves_the_phase_clock() {
        let mut host = ParticleHost::default();
        host.reset_quads(7, [64., 128.]).unwrap();
        for (i, n) in [3f32, 0., -200., -200., 200., 300., 1800.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        for _ in 0..6 {
            expected.next_float();
        }
        let mut effect = Radiate::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.heads.len(), 2);
        assert_eq!(effect.heads[0].trails, 2..4);
        assert_eq!(effect.sprites[0].scale, [0.2, 0.2, 1.]);
        assert_eq!(effect.sprites[6].alpha, 255);
        effect.update(0, &mut random);
        assert_eq!(effect.visible, 1);
        assert_eq!(effect.sprites[2].alpha, 255);
        assert_eq!(effect.sprites[3].phase, 0);
        effect.update(1, &mut random);
        assert_eq!(effect.visible, 2);
        assert_eq!(effect.sprites[0].phase, 0);
        effect.update(0, &mut random);
        let head = &effect.sprites[0];
        assert_eq!([head.age, head.phase_time, head.phase as i32], [1, 1, 2]);
        assert_eq!(head.position, effect.heads[0].path.points[1]);
        assert_eq!(effect.sprites[2].alpha, 127);
        assert_eq!(random.clone().next_float(), expected.clone().next_float());
        effect.update(0, &mut random);
        for _ in 0..6 {
            expected.next_float();
        }
        let head = &effect.sprites[0];
        assert_eq!([head.age, head.phase_time, head.phase as i32], [0, 1, 0]);
        assert_eq!(head.position, effect.heads[0].path.points[0]);
        assert_eq!(effect.sprites[2].alpha, 255);
        assert_eq!(effect.sprites[3].alpha, 0);
        assert_eq!(random.clone().next_integer(), expected.next_integer());
        assert_eq!(random.clone().next_float(), expected.next_float());
        effect.update(0x8000_0000, &mut random);
        assert_eq!(effect.sprites[0].age, i32::MIN);
        for trail in &effect.sprites[2..6] {
            assert_eq!(
                [trail.age, trail.phase as i32, i32::from(trail.alpha)],
                [0; 3]
            );
        }
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[6][0].color, 0xffff_ffff);
    }
}
