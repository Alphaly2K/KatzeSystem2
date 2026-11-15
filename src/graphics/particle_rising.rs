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

pub(super) struct Rising {
    scale: f32,
    image: [f32; 2],
    head: Option<Head>,
    trails: Vec<Trail>,
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
        }
    }

    pub(super) fn update(&mut self, delta: u32, parameters: &[u32; 32], random: &mut ScriptRandom) {
        let Some(head) = &mut self.head else {
            return;
        };
        let elapsed = integer(delta as f32);
        match head.phase {
            0 => head.rotation += head.spin,
            1 => {
                let p = parameters.map(f32::from_bits);
                let start = head.phase_time as f32;
                let end = start + p[5];
                head.position =
                    std::array::from_fn(|i| linear(p[i], p[i + 6], start, end, head.age as f32));
                head.rotation += head.spin;
                if head.age as f32 > end {
                    head.phase = 2;
                    head.phase_time = head.age;
                }
            }
            _ => {}
        }
        let count = self.trails.len();
        if head.phase <= 2 {
            if let Some(trail) = self.trails.iter_mut().find(|trail| !trail.active) {
                let target = head
                    .position
                    .map(|n| (random.next_float() * 500. + n) - 250.);
                trail.path = Some(TwoPointPath::new(
                    [head.position, target],
                    count as f32,
                    100.,
                ));
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
}
