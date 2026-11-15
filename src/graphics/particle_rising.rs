use super::{Transform, particle_curve::TwoPointPath};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn linear(from: f32, to: f32, start: f32, end: f32, time: f32) -> f32 {
    if time <= start {
        from
    } else if time >= end {
        to
    } else {
        ((time - start) / (end - start)) * (to - from) + from
    }
}

fn spin(random: &mut ScriptRandom) -> f32 {
    (random.next_float() as f64 * std::f32::consts::PI as f64 - std::f32::consts::FRAC_PI_2 as f64)
        as f32
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    position: [f32; 3],
    rotation: f32,
    spin: f32,
}

struct Trail {
    age: i32,
    active: bool,
    path: Option<TwoPointPath>,
    position: [f32; 3],
    rotation: f32,
    scale: f32,
    alpha: u32,
}

#[derive(Clone, Copy)]
enum Variant {
    Rising3,
    Rising4,
}

pub(super) struct Rising {
    scale: f32,
    image: [f32; 2],
    head: Option<Head>,
    trails: Vec<Trail>,
    variant: Variant,
}

impl Rising {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1], p[2]];
        let head = (!host.quads.is_empty()).then(|| Head {
            age: 0,
            phase_time: 0,
            phase: 0,
            position: anchor,
            rotation: 0.,
            spin: spin(random),
        });
        let trails = (1..host.quads.len())
            .map(|_| Trail {
                age: 0,
                active: false,
                path: None,
                position: anchor,
                rotation: 0.,
                scale: 1.,
                alpha: 0,
            })
            .collect();
        Self {
            scale: p[3] / 100.,
            image,
            head,
            trails,
            variant: Variant::Rising4,
        }
    }

    pub(super) fn new_rising3(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Self {
        let mut effect = Self::new(host, image, random);
        effect.variant = Variant::Rising3;
        effect
    }

    pub(super) fn update(&mut self, delta: u32, parameters: &[u32; 32], random: &mut ScriptRandom) {
        let Some(head) = &mut self.head else {
            return;
        };
        let elapsed = integer(delta as f32);
        match head.phase {
            0 => {
                head.rotation += head.spin;
                if matches!(self.variant, Variant::Rising3)
                    && head.age as f64 > f32::from_bits(parameters[4]) as f64
                {
                    head.phase = 1;
                    head.phase_time = head.age;
                }
            }
            1 => {
                let p = parameters.map(f32::from_bits);
                let start = head.phase_time as f32;
                let end = start + p[5];
                head.position =
                    std::array::from_fn(|i| linear(p[i], p[i + 6], start, end, head.age as f32));
                head.rotation += head.spin;
                let past_end = match self.variant {
                    Variant::Rising3 => head.age as f64 > head.phase_time as f64 + p[5] as f64,
                    Variant::Rising4 => head.age as f32 > end,
                };
                if past_end {
                    head.phase = 2;
                    head.phase_time = head.age;
                }
            }
            _ => {}
        }
        let count = self.trails.len();
        if head.phase <= 2 {
            if let Some(trail) = self.trails.iter_mut().find(|trail| !trail.active) {
                let target = head.position.map(|n| match self.variant {
                    Variant::Rising3 => {
                        (random.next_float() as f64 * 500. + n as f64 - 250.) as f32
                    }
                    Variant::Rising4 => (random.next_float() * 500. + n) - 250.,
                });
                let points = [head.position, target];
                trail.path = Some(match self.variant {
                    Variant::Rising3 => TwoPointPath::new_extended(points, count as f32, 100.),
                    Variant::Rising4 => TwoPointPath::new(points, count as f32, 100.),
                });
                trail.position = head.position;
                trail.rotation = head.rotation;
                trail.scale = self.scale;
                trail.active = true;
                // 新拖尾重抽主体角速度，拖尾自身保留出生时的旋转。
                head.spin = spin(random);
            }
        }
        for trail in &mut self.trails {
            if !trail.active {
                continue;
            }
            trail.scale *= 0.9;
            trail.position = trail.path.as_ref().unwrap().at(trail.age as f32);
            trail.alpha =
                integer(linear(255., 0., 0., count as f32, trail.age as f32)) as u32 & 255;
            trail.age = trail.age.wrapping_add(elapsed);
            if trail.age >= count as i32 {
                trail.age = 0;
                trail.active = false;
            }
        }
        head.age = head.age.wrapping_add(elapsed);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let Some(head) = &self.head else {
            return;
        };
        for (index, quad) in host.quads.iter_mut().enumerate() {
            let (position, rotation, scale, alpha) = if index == 0 {
                (head.position, head.rotation, self.scale, 255)
            } else {
                let p = &self.trails[index - 1];
                (p.position, p.rotation, p.scale, p.alpha)
            };
            let mut transform = Transform::default();
            transform.set_scale([scale, scale, 1.]);
            transform.set_rotation([0., 0., rotation]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&position);
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
                v.color = 0xffffff | (alpha << 24);
                v.uv = [(i & 1) as f32 * 0.5, 0.5 + (i >> 1) as f32 * 0.5];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(count: usize, parameters: &[f32]) -> crate::Result<ParticleHost> {
        let mut host = ParticleHost::default();
        host.reset_quads(count, [64., 128.])?;
        for (i, value) in parameters.iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        Ok(host)
    }

    #[test]
    fn zero_time_updates_spawn_trails_shrink_them_and_replace_the_head_spin() -> crate::Result<()> {
        let mut host = host(8, &[580., 500., 0., 40., 0., 0., 580., -40., 0.])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.head.as_ref().unwrap().spin.to_bits(), 0x3e8cd0de);
        effect.update(0, &host.parameters, &mut random);
        let head = effect.head.as_ref().unwrap();
        assert_eq!(
            (head.age, head.rotation.to_bits(), head.spin.to_bits()),
            (0, 0x3e8cd0de, 0xbfc34255)
        );
        let trail = &effect.trails[0];
        assert!(trail.active);
        assert_eq!(
            (trail.age, trail.scale.to_bits(), trail.alpha),
            (0, 0x3eb851eb, 255)
        );
        assert_eq!(
            trail.path.as_ref().unwrap().points[1].map(f32::to_bits),
            [0x43ca9e0e, 0x44060bc6, 0xc3758416]
        );
        effect.update(0, &host.parameters, &mut random);
        let head = effect.head.as_ref().unwrap();
        assert_eq!(
            (head.age, head.rotation.to_bits(), head.spin.to_bits()),
            (0, 0xbfa00e1e, 0xbe818dd2)
        );
        assert_eq!(effect.trails.iter().filter(|p| p.active).count(), 2);
        assert_eq!(effect.trails[0].age, 0);
        assert!(effect.trails[0].scale < effect.trails[1].scale);
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..9 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        effect.write_mesh(&mut host);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.5], [0.5, 1.]]
        );
        assert_eq!(host.quads[1][0].color, 0xffffffff);
        assert_eq!(host.quads[3][0].color, 0x00ffffff);
        Ok(())
    }

    #[test]
    fn reclaimed_trails_keep_their_last_alpha_and_position_until_reused() -> crate::Result<()> {
        let host = host(3, &[580., 500., 0., 40.])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new(&host, [64., 128.], &mut random);
        effect.update(1, &host.parameters, &mut random);
        assert!(effect.trails[0].active);
        assert_eq!(effect.trails[0].alpha, 255);
        effect.update(1, &host.parameters, &mut random);
        assert!(!effect.trails[0].active);
        assert_eq!((effect.trails[0].age, effect.trails[0].alpha), (0, 127));
        let previous = (
            effect.trails[0].position,
            effect.trails[0].scale,
            effect.trails[0].rotation,
        );
        effect.head.as_mut().unwrap().phase = 3;
        effect.update(1, &host.parameters, &mut random);
        assert!(effect.trails.iter().all(|p| !p.active && p.alpha == 127));
        effect.update(10, &host.parameters, &mut random);
        let p = &effect.trails[0];
        assert_eq!((p.position, p.scale, p.rotation), previous);
        assert_eq!(p.alpha, 127);
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..9 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }

    #[test]
    fn empty_and_single_head_groups_preserve_randomness_and_motion_phase_boundaries()
    -> crate::Result<()> {
        let empty = host(0, &[])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new(&empty, [64., 128.], &mut random);
        effect.update(100, &empty.parameters, &mut random);
        let mut expected = ScriptRandom::seeded(17);
        assert_eq!(random.next_float(), expected.next_float());
        let mut host = host(1, &[1., 3., 5., 25., 0., 4., 9., 11., 13.])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new(&host, [64., 128.], &mut random);
        let head = effect.head.as_mut().unwrap();
        head.phase = 1;
        head.phase_time = 3;
        head.age = 5;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().position, [5., 7., 9.]);
        host.parameters[7] = 19f32.to_bits();
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().position, [5., 11., 9.]);
        effect.head.as_mut().unwrap().age = 7;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().position, [9., 19., 13.]);
        assert_eq!(effect.head.as_ref().unwrap().phase, 1);
        effect.head.as_mut().unwrap().age = 8;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(
            (
                effect.head.as_ref().unwrap().phase,
                effect.head.as_ref().unwrap().phase_time
            ),
            (2, 8)
        );
        let mut expected = ScriptRandom::seeded(17);
        expected.next_float();
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }

    #[test]
    fn rising3_moves_after_delay_and_keeps_extended_paths_and_shared_randomness()
    -> crate::Result<()> {
        use crate::graphics::{ParticleMesh, Projection};
        let mut host = host(8, &[640., 360., 0., 80., 1., 15., 640., -80., 0.])?;
        host.surface_size = [1280, 720];
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new_rising3(&host, [64., 128.], &mut random);
        for step in 1u32..160 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, &host.parameters, &mut random);
            let expected = match step {
                1 => Some((
                    [
                        1, 0, 0, 0x44200000, 0x43b40000, 0x3e8cd0de, 1, 1, 0x44200000, 0x43b40000,
                        0, 0x4397c3cb, 255,
                    ],
                    5,
                )),
                3 => Some((
                    [
                        3, 2, 1, 0x44200000, 0x43b40000, 0xbfc07192, 3, 1, 0x44161445, 0x43b81bab,
                        0xc25efe59, 0x4397c3cb, 182,
                    ],
                    13,
                )),
                12 => Some((
                    [
                        17, 2, 1, 0x44200000, 0xc24aaaa8, 0x3e48bb84, 3, 1, 0x44135522, 0x4211103d,
                        0x3fbe842c, 0x4378a433, 182,
                    ],
                    45,
                )),
                40 => Some((
                    [
                        59, 18, 2, 0x44200000, 0xc2a00000, 0x3eb6ee1e, 4, 1, 0x44195718,
                        0xc1d7518b, 0x41b4b1e7, 0x43188f62, 145,
                    ],
                    141,
                )),
                159 => Some((
                    [
                        241, 18, 2, 0x44200000, 0xc2a00000, 0x3eb6ee1e, 5, 1, 0x44051500,
                        0xc28e3951, 0x416453b1, 0x433979c4, 109,
                    ],
                    561,
                )),
                _ => None,
            };
            if let Some((expected, floats)) = expected {
                let h = effect.head.as_ref().unwrap();
                let t = &effect.trails[0];
                assert_eq!(
                    [
                        h.age as u32,
                        h.phase_time as u32,
                        h.phase,
                        h.position[0].to_bits(),
                        h.position[1].to_bits(),
                        h.rotation.to_bits(),
                        t.age as u32,
                        u32::from(t.active),
                        t.position[0].to_bits(),
                        t.position[1].to_bits(),
                        t.position[2].to_bits(),
                        t.path.as_ref().unwrap().length.to_bits(),
                        t.alpha,
                    ],
                    expected,
                    "step {step}"
                );
                let mut expected = ScriptRandom::seeded(17);
                for _ in 0..floats {
                    expected.next_float();
                }
                let mut actual = random.clone();
                assert_eq!(actual.next_integer(), expected.next_integer());
                assert_eq!(
                    actual.next_float().to_bits(),
                    expected.next_float().to_bits()
                );
            }
            if step == 40 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.)?;
                for (index, expected) in [
                    [0x441e7b12, 0xc308eb7c, 0x3f6c4ec5, 0x3a83126f, 0xffffffff],
                    [0x44182a9f, 0xc2942098, 0x3f6c3800, 0x3a8619ce, 0x91ffffff],
                ]
                .into_iter()
                .enumerate()
                {
                    let q = mesh.projected_quad(index, &Projection::default()).unwrap();
                    assert_eq!(
                        [
                            q[0].position[0].to_bits(),
                            q[0].position[1].to_bits(),
                            q[0].position[2].to_bits(),
                            q[0].rhw.to_bits(),
                            q[0].color
                        ],
                        expected
                    );
                    assert_eq!([q[0].uv, q[3].uv], [[0., 0.5], [0.5, 1.]]);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn rising3_reads_live_targets_and_preserves_integer_phase_deadlines() -> crate::Result<()> {
        let mut host = host(1, &[1., 3., 5., 25., 2., 4., 9., 11., 13.])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rising::new_rising3(&host, [64., 128.], &mut random);
        effect.head.as_mut().unwrap().age = 2;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().phase, 0);
        effect.head.as_mut().unwrap().age = 3;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().phase, 1);
        effect.head.as_mut().unwrap().age = 5;
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().position, [5., 7., 9.]);
        host.parameters[7] = 19f32.to_bits();
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().position, [5., 11., 9.]);
        let head = effect.head.as_mut().unwrap();
        head.phase_time = 16_777_216;
        head.age = 16_777_217;
        host.parameters[5] = 0.5f32.to_bits();
        effect.update(0, &host.parameters, &mut random);
        assert_eq!(effect.head.as_ref().unwrap().phase, 2);
        let mut expected = ScriptRandom::seeded(17);
        expected.next_float();
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(
            random.next_float().to_bits(),
            expected.next_float().to_bits()
        );
        Ok(())
    }
}
