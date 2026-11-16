use super::Transform;
use crate::kcs::{ParticleHost, transform_vector};
use crate::{Error, Result};
use std::ops::Range;

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

#[derive(Clone)]
struct Sprite {
    age: i32,
    phase_time: i32,
    phase: u32,
    position: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 2],
    alpha: u8,
}

struct Head {
    index: usize,
    trails: Range<usize>,
}

pub(super) struct Ring {
    anchor: [f32; 2],
    center: [f32; 2],
    scale: [f32; 2],
    lives: [i32; 2],
    frequency: u32,
    counter: u32,
    visible: usize,
    image: [f32; 2],
    heads: Vec<Head>,
    sprites: Vec<Sprite>,
}

impl Ring {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Result<Self> {
        let p = host.parameters.map(f32::from_bits);
        let frequency = integer(p[5]);
        if frequency == 0 {
            return Err(Error::new(0, "环形粒子激活间隔为零"));
        }
        let width = integer(p[0]) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(width)
            .unwrap_or(host.quads.len());
        let scale = [p[3], p[4]].map(|n| {
            let n = n / 100.;
            if n < 0.001 { 0.001 } else { n }
        });
        let mut effect = Self {
            anchor: [p[1], p[2]],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            scale,
            lives: [integer(p[6]), integer(p[7])],
            frequency: if frequency < 0 { 1 } else { frequency as u32 },
            counter: 0,
            visible: 0,
            image,
            heads: Vec::with_capacity(count),
            sprites: vec![
                Sprite {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    position: [0.; 3],
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
            effect.heads.push(Head {
                index,
                trails: first..first + trails,
            });
            let head = &mut effect.sprites[index];
            head.pivot = image.map(|n| n * 0.5);
            head.position = [p[1], p[2], 0.];
            head.scale = [scale[0], scale[0], 1.];
            head.alpha = 0;
            for i in 0..trails {
                let trail = &mut effect.sprites[first + i];
                trail.pivot = image.map(|n| n * 0.5);
                trail.position = [p[1], p[2], (i + 1) as f32];
                trail.alpha = 0;
            }
        }
        Ok(effect)
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if self.counter.is_multiple_of(self.frequency) {
            self.visible = (self.visible + 1).min(self.heads.len());
        }
        self.counter = self.counter.wrapping_add(1);
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for group in &self.heads[..self.visible] {
            let head = &mut self.sprites[group.index];
            head.position[..2].copy_from_slice(&self.anchor);
            if head.phase <= 1 {
                let time = head.age.wrapping_sub(head.phase_time);
                let scale = fade(
                    self.scale[0],
                    self.scale[1],
                    self.lives[0].wrapping_add(self.lives[1]) as f32,
                    head.age as f32,
                );
                head.scale[..2].fill(scale);
                head.alpha = integer(fade(
                    if head.phase == 0 { 0. } else { 200. },
                    if head.phase == 0 { 200. } else { 0. },
                    self.lives[head.phase as usize] as f32,
                    time as f32,
                )) as u8;
                if time > self.lives[head.phase as usize] {
                    head.phase = if head.phase == 0 { 1 } else { 9 };
                    head.phase_time = head.age;
                }
            }
            let head = head.clone();
            if head.phase <= 3 {
                if let Some(trail) = self.sprites[group.trails.clone()]
                    .iter_mut()
                    .find(|p| p.phase == 0)
                {
                    trail.pivot = head.pivot;
                    trail.position[..2].copy_from_slice(&head.position[..2]);
                    trail.scale = head.scale;
                    trail.alpha = 0;
                    trail.phase = 1;
                    trail.age = 0;
                    trail.phase_time = 0;
                }
            }
            for trail in &mut self.sprites[group.trails.clone()] {
                if trail.phase != 0 {
                    trail.position[..2].copy_from_slice(&head.position[..2]);
                    trail.alpha = integer(fade(10., 0., 40., trail.age as f32)) as u8;
                    trail.age = trail.age.wrapping_add(1);
                    if trail.age >= 40 {
                        trail.alpha = 0;
                    }
                }
            }
            let head = &mut self.sprites[group.index];
            for (position, offset) in head.position[..2].iter_mut().zip(offset) {
                *position += offset;
            }
            head.age = head.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        for (p, quad) in self.sprites.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale(p.scale);
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
    fn grouped_activation_strict_phases_and_trails_keep_their_separate_clocks_and_positions() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(20, [64., 128.]).unwrap();
        for (i, n) in [5f32, 420., 420., 1., 500., 5., 1., 25.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut effect = Ring::new(&host, [64., 128.]).unwrap();
        assert_eq!(effect.heads.len(), 4);
        assert_eq!(effect.heads[0].trails, 4..8);
        assert_eq!(effect.sprites[7].position, [420., 420., 4.]);
        assert_eq!(effect.sprites[4].scale, [1.; 3]);
        let steps = [
            0,
            1,
            2,
            4,
            7,
            11,
            0,
            1,
            17,
            30,
            60,
            120,
            0x80000000,
            u32::MAX,
        ];
        for i in 0..59 {
            effect.update(
                steps.get(i).copied().unwrap_or(1),
                [
                    640. + (i as i32 % 3 - 1) as f32 * 7.25,
                    360. + (i as i32 % 5 - 2) as f32 * -3.5,
                ],
            );
            let head = &effect.sprites[0];
            let expected = match i {
                0 => [0, 0, 0, 0],
                2 => [3, 0, 0, 200],
                3 => [7, 3, 1, 200],
                13 => [253, 43, 9, 0],
                58 => [298, 43, 9, 0],
                _ => continue,
            };
            assert_eq!(
                [
                    head.age as u32,
                    head.phase_time as u32,
                    head.phase,
                    u32::from(head.alpha)
                ],
                expected
            );
            if i == 0 {
                assert_eq!(head.position, [412.75, 427., 0.]);
                assert_eq!(effect.sprites[4].position, [420., 420., 1.]);
                assert_eq!(effect.sprites[4].alpha, 10);
            }
        }
        assert_eq!(effect.visible, 4);
        let trail = &effect.sprites[4];
        assert_eq!(
            [trail.age as u32, trail.phase, u32::from(trail.alpha)],
            [59, 1, 0]
        );
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        host.parameters[5] = 0;
        assert!(Ring::new(&host, [64., 128.]).is_err());
    }
}
