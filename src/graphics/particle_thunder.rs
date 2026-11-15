use crate::kcs::ParticleHost;
use crate::kcs::ScriptRandom;
use std::sync::atomic::{AtomicU32, Ordering};

struct Sprite {
    position: [f32; 3],
    scale: [f32; 3],
    alpha: u8,
    region: u32,
}

struct Trail {
    age: u32,
    active: bool,
    sprite: Sprite,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    sprite: Sprite,
    trails: Vec<Trail>,
}

pub(super) struct Thunder {
    anchor: [f32; 2],
    center: [f32; 2],
    image: [f32; 2],
    lives: [i32; 2],
    active: usize,
    updates: u32,
    finished: usize,
    heads: Vec<Head>,
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&(value as f64)) {
        value as i32
    } else {
        i32::MIN
    }
}

fn linear(from: f32, to: f32, duration: f32, time: f32) -> u8 {
    integer(if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    }) as u8
}

impl Thunder {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: &AtomicU32,
    ) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = integer(p[0]) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(group)
            .unwrap_or(host.quads.len());
        let scale = if p[3] / 100. < 0.001 {
            0.001
        } else {
            p[3] / 100.
        };
        let anchor = [p[1], p[2]];
        let heads = (0..count)
            .map(|_| {
                // 图块使用模块自己的 C 随机流，缩放才消费宿主浮点随机流。
                let previous = module_state
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        Some(n.wrapping_mul(214013).wrapping_add(2531011))
                    })
                    .unwrap();
                let region = (previous.wrapping_mul(214013).wrapping_add(2531011) >> 16) & 3;
                let scale = [
                    (((random.next_float() * 0.5) * scale) as f64 + scale as f64 * 0.7) as f32,
                    scale * 3.,
                    1.,
                ];
                let sprite = |depth| Sprite {
                    position: [anchor[0], anchor[1], depth],
                    scale,
                    alpha: 0,
                    region,
                };
                Head {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    sprite: sprite(0.),
                    trails: (1..group)
                        .map(|i| Trail {
                            age: 0,
                            active: false,
                            sprite: Sprite {
                                scale: [1.; 3],
                                ..sprite(i as f32)
                            },
                        })
                        .collect(),
                }
            })
            .collect();
        Self {
            anchor,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            lives: [integer(p[5]), integer(p[6])],
            active: 0,
            updates: 0,
            finished: 0,
            heads,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        self.updates = self.updates.wrapping_add(1);
        if self.updates.is_multiple_of(2) {
            self.active = (self.active + 1).min(self.heads.len());
        }
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for head in &mut self.heads[..self.active] {
            head.sprite.position[..2].copy_from_slice(&self.anchor);
            if head.phase <= 1 {
                let elapsed = head.age.wrapping_sub(head.phase_time);
                let duration = self.lives[head.phase as usize];
                let (from, to) = if head.phase == 0 {
                    (0., 255.)
                } else {
                    (255., 0.)
                };
                head.sprite.alpha = linear(from, to, duration as f32, elapsed as f32);
                if elapsed > duration {
                    head.phase_time = head.age;
                    if head.phase == 0 {
                        head.phase = 1;
                    } else {
                        head.phase = 9;
                        self.finished += 1;
                    }
                }
            }
            if head.phase <= 3 {
                if let Some(trail) = head.trails.iter_mut().find(|trail| !trail.active) {
                    trail.age = 0;
                    trail.active = true;
                    trail.sprite.position[..2].copy_from_slice(&head.sprite.position[..2]);
                    trail.sprite.scale = head.sprite.scale;
                }
            }
            let count = head.trails.len() as u32;
            for trail in &mut head.trails {
                if trail.active {
                    trail.sprite.position[..2].copy_from_slice(&self.anchor);
                    trail.sprite.alpha = linear(count as f32, 0., count as f32, trail.age as f32);
                    trail.age = trail.age.wrapping_add(1);
                    if trail.age >= count {
                        trail.sprite.alpha = 0;
                        trail.active = false;
                    }
                }
            }
            for (position, offset) in head.sprite.position[..2].iter_mut().zip(offset) {
                *position += offset;
            }
            head.age = head.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        host.active = self.finished < self.heads.len();
        for (index, head) in self.heads.iter().enumerate() {
            for (i, sprite) in std::iter::once(&head.sprite)
                .chain(head.trails.iter().map(|t| &t.sprite))
                .enumerate()
            {
                let quad = if i == 0 {
                    index
                } else {
                    self.heads.len() + index * head.trails.len() + i - 1
                };
                for (j, vertex) in host.quads[quad].iter_mut().enumerate() {
                    vertex.position = std::array::from_fn(|axis| {
                        let point = if axis < 2 {
                            ((j >> axis) & 1) as f32 * self.image[axis] - self.image[axis] * 0.5
                        } else {
                            0.
                        };
                        point * sprite.scale[axis] + sprite.position[axis]
                    });
                    vertex.rhw = 1.;
                    vertex.color = (u32::from(sprite.alpha) << 24) | 0xffffff;
                    vertex.uv = [
                        (sprite.region + (j & 1) as u32) as f32 * 0.25,
                        (j >> 1) as f32,
                    ];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_flash_keeps_strict_phases_update_based_trails_and_module_randomness()
    -> crate::Result<()> {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            active: true,
            ..Default::default()
        };
        for (i, n) in [3f32, 125., -70., 65., 4., 3., 5.].into_iter().enumerate() {
            host.parameters[i] = n.to_bits();
        }
        host.reset_quads(7, [64., 128.])?;
        let remainder = host.quads[6];
        let state = AtomicU32::new(1);
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        expected.next_float();
        expected.next_float();
        let mut effect = Thunder::new(&host, [64., 128.], &mut random, &state);
        assert_eq!(state.load(Ordering::Relaxed), 3_357_800_067);
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        effect.write_mesh(&mut host);
        assert_eq!(
            host.quads[0][0].position.map(f32::to_bits),
            [0x42d0_a8b8, 0xc342_cccc, 0]
        );
        assert_eq!(host.quads[2][0].position, [93., -134., 1.]);
        assert_eq!(host.quads[0][0].uv, [0.25, 0.]);
        assert_eq!(host.quads[1][0].uv, [0.75, 0.]);
        assert_eq!(host.quads[6], remainder);

        effect.update(0, [640., 360.]);
        assert_eq!(effect.active, 0);
        effect.update(1, [642.5, 358.75]);
        assert_eq!(effect.active, 1);
        assert_eq!(effect.heads[0].sprite.position, [127.5, -71.25, 0.]);
        assert_eq!(effect.heads[0].trails[0].sprite.position, [125., -70., 1.]);
        assert_eq!(effect.heads[0].trails[0].sprite.alpha, 2);
        effect.update(2, [645., 357.5]);
        assert_eq!(effect.heads[0].sprite.alpha, 85);
        assert!(!effect.heads[0].trails[0].active);
        assert_eq!(effect.heads[0].trails[1].sprite.alpha, 2);
        effect.update(4, [647.5, 356.25]);
        assert_eq!(effect.heads[0].phase, 0);
        assert_eq!(effect.heads[0].sprite.alpha, 255);
        effect.update(7, [650., 355.]);
        assert_eq!(effect.heads[0].phase, 1);
        effect.update(11, [652.5, 353.75]);
        effect.write_mesh(&mut host);
        assert_eq!(effect.finished, 2);
        assert!(!host.active);
        assert_eq!(host.quads[6], remainder);

        let restarted = Thunder::new(&host, [64., 128.], &mut random, &state);
        assert_eq!(state.load(Ordering::Relaxed), 3_884_216_597);
        assert_eq!(restarted.heads[0].sprite.region, 2);
        assert_eq!(restarted.heads[1].sprite.region, 0);
        Ok(())
    }
}
