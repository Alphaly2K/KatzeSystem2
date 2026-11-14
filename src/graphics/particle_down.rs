use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

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
struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    origin: [f32; 2],
    position: [f32; 3],
    scale: [f32; 2],
    target: f32,
    alpha: u32,
}

pub(super) struct Down {
    anchor: [f32; 2],
    center: [f32; 2],
    scale: f32,
    lives: [i32; 6],
    image: [f32; 2],
    visible: usize,
    ticks: i32,
    particles: Vec<Particle>,
}

impl Down {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1]];
        let mut scale = p[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let offset = [p[3], p[4]];
        let mut particles = Vec::with_capacity(host.quads.len());
        for i in 0..host.quads.len() {
            let (origin, size, target) = if i == 0 {
                (
                    std::array::from_fn(|j| anchor[j] + offset[j]),
                    [scale; 2],
                    0.,
                )
            } else {
                // 纵向随机偏移的乘加只在写回时舍入。
                let origin = [
                    (i as i32).wrapping_mul(50) as f32 * scale + anchor[0],
                    (f64::from(random.next_float()) * f64::from(scale * 20.) + f64::from(anchor[1]))
                        as f32,
                ];
                let target = (random.next_float() * 0.3 + 0.8) * scale;
                (origin, [scale * 0.025, scale * 0.1], target)
            };
            particles.push(Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                origin,
                position: [origin[0], origin[1], i as f32],
                scale: size,
                target,
                alpha: 0,
            });
        }
        Self {
            anchor,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            scale,
            lives: std::array::from_fn(|i| integer(p[i + 5])),
            image,
            visible: 1,
            ticks: 0,
            particles,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if self.particles.is_empty() {
            return;
        }
        // 逐次更新计数决定下一条竖线何时出现，与时间增量独立。
        if self.ticks > 5 {
            self.visible = (self.visible + 1).min(self.particles.len());
            self.ticks = 0;
        }
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for (index, p) in self.particles.iter_mut().enumerate().take(self.visible) {
            let time = p.age.wrapping_sub(p.phase_time);
            if index == 0 {
                p.position[..2].copy_from_slice(&if p.phase < 3 { p.origin } else { self.anchor });
                if p.phase < 2 {
                    let duration = self.lives[0].wrapping_add(self.lives[1]) as f32;
                    let time = p.age as f32;
                    p.scale = [
                        fade(self.scale * 0.01, self.scale * 2.5, duration, time),
                        fade(self.scale * 0.01, self.scale * 1.8, duration, time),
                    ];
                    p.alpha = integer(fade(0., 128., duration, time)) as u32 & 255;
                    // 主体的两个阶段都比较累计年龄，竖线则比较各阶段年龄。
                    if p.age > self.lives[p.phase as usize] {
                        p.phase += 1;
                        p.phase_time = p.age;
                    }
                }
            } else {
                p.position[..2].copy_from_slice(&if p.phase < 3 { p.origin } else { self.anchor });
                let duration = self.lives[usize::from(p.phase != 0)] as f32;
                match p.phase {
                    0 => {
                        p.scale = [
                            fade(self.scale * 0.025, self.scale * 0.25, duration, time as f32),
                            fade(self.scale * 0.1, p.target * 1.2, duration, time as f32),
                        ];
                        p.alpha = integer(fade(0., 255., duration, time as f32)) as u32 & 255;
                    }
                    1 => p.scale[1] = fade(self.scale * 1.2, p.target, duration, time as f32),
                    3 => {
                        let scale = fade(self.scale, self.scale * 1.05, duration, time as f32);
                        p.scale = [scale; 2];
                    }
                    4 => {
                        let scale = fade(self.scale * 1.05, self.scale, duration, time as f32);
                        p.scale = [scale; 2];
                    }
                    _ => {}
                }
                let boundary = match p.phase {
                    0 => Some(0),
                    1 => Some(1),
                    3 => Some(3),
                    4 => Some(4),
                    _ => None,
                };
                if boundary.is_some_and(|i| time > self.lives[i]) {
                    p.phase += 1;
                    p.phase_time = p.age;
                }
            }
            for (i, n) in offset.into_iter().enumerate() {
                p.position[i] += n;
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
        self.ticks = self.ticks.wrapping_add(1);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (index, (p, quad)) in self.particles.iter().zip(&mut host.quads).enumerate() {
            let mut matrix = [0.; 16];
            matrix[0] = p.scale[0];
            matrix[5] = p.scale[1];
            matrix[10] = 1.;
            matrix[15] = 1.;
            matrix[12..15].copy_from_slice(&p.position);
            for (i, v) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (i >> 1) as f32 * self.image[1]
                            - if index == 0 { self.image[1] * 0.5 } else { 0. },
                        0.,
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = 0x00ff_ffff | (p.alpha << 24);
                v.uv = if index == 0 {
                    [0.25 + (i & 1) as f32 * 0.75, (i >> 1) as f32 * 0.5]
                } else {
                    [(i & 1) as f32 * 0.25, (i >> 1) as f32]
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_keeps_staggered_activation_absolute_head_age_and_random_rounding() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(5, [64., 128.]).unwrap();
        for (i, value) in [1125f32, 19., 27., -60., 80., 13., 5., 80., 5., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Down::new(&host, [64., 128.], &mut ScriptRandom::seeded(17));
        assert_eq!(effect.particles[1].origin[1].to_bits(), 0x41b1_61c7);
        for step in 1u32..=40 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, [640., 360.]);
            let expected = match step {
                7 => [2, 6, 0, 0, 42, 0x3e68_3e44, 0x3e27_bb30],
                17 => [3, 21, 15, 2, 106, 0x3f10_1d7e, 0x3ecf_9724],
                40 => [5, 59, 15, 2, 106, 0x3f10_1d7e, 0x3ecf_9724],
                _ => continue,
            };
            let head = &effect.particles[0];
            assert_eq!(
                [
                    effect.visible as u32,
                    head.age as u32,
                    head.phase_time as u32,
                    head.phase,
                    head.alpha,
                    head.scale[0].to_bits(),
                    head.scale[1].to_bits(),
                ],
                expected
            );
        }
        assert_eq!(effect.particles[1].phase, 2);
        assert_eq!(effect.particles[1].alpha, 255);
        assert_eq!(effect.particles[1].scale[1].to_bits(), 0x3e69_aa22);

        let mut effect = Down::new(&host, [64., 128.], &mut ScriptRandom::seeded(17));
        for _ in 0..7 {
            effect.update(0, [640., 360.]);
        }
        assert_eq!(effect.visible, 2);
        assert!(effect.particles.iter().all(|p| p.age == 0));
    }
}
