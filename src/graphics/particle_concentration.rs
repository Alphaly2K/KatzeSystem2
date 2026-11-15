use super::{Transform, particle_curve::TwoPointPath};
use crate::kcs::{ParticleHost, ScriptRandom, normalize_vector, transform_vector};
use crate::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

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

struct Trail {
    age: u32,
    active: bool,
    position: [f32; 3],
    scale: [f32; 2],
    angle: f32,
    centered: bool,
    alpha: u32,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: TwoPointPath,
    position: [f32; 3],
    previous: [f32; 3],
    scale: f32,
    lifetime: f32,
    trails: Vec<Trail>,
}

pub(super) struct Concentration {
    duration: f32,
    scale: f32,
    center: [f32; 2],
    extent: [f32; 2],
    image: [f32; 2],
    visible: Arc<AtomicU32>,
    heads: Vec<Head>,
}

impl Concentration {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        visible: Arc<AtomicU32>,
    ) -> Result<Self> {
        let p = host.parameters.map(f32::from_bits);
        let group = integer(p[0]) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(group)
            .unwrap_or(host.quads.len());
        if count != 0 && [p[5], p[6]].into_iter().any(|n| integer(n) == 0) {
            return Err(Error::new(0, "集中线的散布尺寸整数部分不能为零"));
        }
        let mut effect = Self {
            duration: p[1],
            scale: p[2] / 1000.,
            center: [p[3], p[4]],
            extent: [p[5], p[6]],
            image,
            visible,
            heads: Vec::with_capacity(count),
        };
        for _ in 0..count {
            let mut head = effect.spawn(random);
            for _ in 0..group.saturating_sub(1) {
                head.trails.push(Trail {
                    age: 0,
                    active: false,
                    position: head.position,
                    scale: [1.; 2],
                    angle: 0.,
                    centered: false,
                    alpha: 0,
                });
            }
            effect.heads.push(head);
        }
        Ok(effect)
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Head {
        let scale = (random.next_float() * self.scale) * 0.2 + self.scale * 0.8;
        let point = std::array::from_fn::<_, 2, _>(|i| {
            let sample = random.next_integer() % integer(self.extent[i]) as u32;
            (sample as f32 - self.extent[i] * 0.5) + self.center[i]
        });
        let position = [point[0], point[1], 500.];
        let path = TwoPointPath::new(
            [position, [self.center[0], self.center[1], 500.]],
            self.duration,
            100.,
        );
        let lifetime = (random.next_float() as f64 * (self.duration as f64 * 0.2)
            + (self.duration * 0.6) as f64) as f32;
        Head {
            age: 0,
            phase_time: 0,
            phase: 0,
            path,
            position,
            previous: position,
            scale,
            lifetime,
            trails: Vec::new(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let mut visible = self.visible.load(Ordering::Relaxed);
        if random.next_integer().is_multiple_of(10) {
            visible = visible.wrapping_add(10);
        }
        visible = visible.min(self.heads.len() as u32);
        self.visible.store(visible, Ordering::Relaxed);
        let elapsed = integer(delta as f32);
        for i in 0..visible as usize {
            if self.heads[i].phase > 1 {
                let mut next = self.spawn(random);
                next.phase_time = self.heads[i].phase_time;
                next.trails = std::mem::take(&mut self.heads[i].trails);
                for trail in &mut next.trails {
                    trail.age = 0;
                    trail.active = false;
                    trail.position = next.position;
                    trail.alpha = 0;
                }
                self.heads[i] = next;
            } else {
                let head = &mut self.heads[i];
                head.position = head.path.at(head.age as f32);
                let lifetime = if head.phase == 0 {
                    head.lifetime
                } else {
                    self.duration
                };
                if head.age as f32 > lifetime {
                    head.phase_time = head.age;
                    head.phase = 2;
                }
            }
            let head = &mut self.heads[i];
            if head.phase == 0 {
                if let Some(trail) = head.trails.iter_mut().find(|trail| !trail.active) {
                    let vector =
                        std::array::from_fn::<_, 3, _>(|j| head.position[j] - head.previous[j]);
                    let distance = length(vector);
                    trail.position = head.position;
                    trail.scale = [head.scale; 2];
                    if distance != 0. {
                        let normal = normalize_vector([vector[0], vector[1], vector[2], 0.]);
                        let dot = (normal[0] * 0. + normal[1]) + normal[2] * 0.;
                        trail.angle = (dot as f64).acos() as f32;
                        if (vector[1] * 0. + vector[0]) + vector[2] * 0. > 0. {
                            trail.angle *= -1.;
                        }
                        trail.position =
                            std::array::from_fn(|j| (head.previous[j] + head.position[j]) * 0.5);
                        trail.scale[1] = (distance * 0.7) * head.scale;
                    }
                    trail.centered = true;
                    trail.active = true;
                }
            }
            let duration = head.trails.len() as f32;
            let count = head.trails.len() as u32;
            for trail in &mut head.trails {
                if !trail.active {
                    continue;
                }
                let time = trail.age as i32 as f32;
                let alpha = if time <= 0. {
                    255.
                } else if time >= duration {
                    0.
                } else {
                    (time / duration) * -255. + 255.
                };
                trail.alpha = integer(alpha) as u32 & 255;
                trail.age = trail.age.wrapping_add(elapsed as u32);
                if trail.age >= count {
                    trail.age = 0;
                    trail.active = false;
                    trail.alpha = 0;
                }
            }
            head.previous = head.position;
            head.age = head.age.wrapping_add(elapsed);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let count = self.heads.len();
        for (index, head) in self.heads.iter().enumerate() {
            Self::write_quad(
                &mut host.quads[index],
                self.image,
                head.position,
                [head.scale; 2],
                0.,
                true,
                0,
            );
            for (j, trail) in head.trails.iter().enumerate() {
                let at = count + index * head.trails.len() + j;
                Self::write_quad(
                    &mut host.quads[at],
                    self.image,
                    trail.position,
                    trail.scale,
                    trail.angle,
                    trail.centered,
                    trail.alpha,
                );
            }
        }
    }

    fn write_quad(
        quad: &mut [crate::kcs::ParticleVertex; 4],
        image: [f32; 2],
        position: [f32; 3],
        scale: [f32; 2],
        angle: f32,
        centered: bool,
        alpha: u32,
    ) {
        let mut transform = Transform::default();
        transform.set_scale([scale[0], scale[1], 1.]);
        transform.set_rotation([0., 0., angle]);
        let mut matrix = *transform.matrix();
        matrix[12..15].copy_from_slice(&position);
        for (i, vertex) in quad.iter_mut().enumerate() {
            let point = transform_vector(
                [
                    (i & 1) as f32 * image[0] - if centered { image[0] * 0.5 } else { 0. },
                    (i >> 1) as f32 * image[1] - if centered { image[1] * 0.5 } else { 0. },
                    0.,
                    1.,
                ],
                matrix,
            );
            vertex.position.copy_from_slice(&point[..3]);
            vertex.rhw = point[3];
            vertex.color = 0xffffff | (alpha << 24);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentration_recycles_trails_on_elapsed_time_and_retains_respawn_geometry() -> Result<()> {
        let mut host = ParticleHost::default();
        host.reset_quads(9, [64., 128.])?;
        for (i, n) in [4f32, 50., 15., 800., 345., 1500., 900.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let remainder = host.quads[8];
        let activation = Arc::new(AtomicU32::new(1));
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Concentration::new(&host, [64., 128.], &mut random, activation.clone())?;
        assert_eq!(effect.heads.len(), 2);
        assert_eq!(effect.heads[0].trails.len(), 3);
        assert!(
            effect.heads[0]
                .trails
                .iter()
                .all(|trail| !trail.centered && trail.scale == [1.; 2])
        );
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..4 {
            expected.next_integer();
            expected.next_float();
        }
        for _ in 0..3 {
            effect.update(0, &mut random);
            expected.next_integer();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        assert!(
            effect.heads[0]
                .trails
                .iter()
                .all(|trail| trail.active && trail.age == 0 && trail.alpha == 255)
        );
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[8], remainder);
        assert_eq!(host.quads[0][0].color, 0xffffff);
        assert_eq!(host.quads[2][0].color, u32::MAX);
        assert_eq!(
            [host.quads[2][0].uv, host.quads[2][3].uv],
            [[0., 0.], [1., 1.]]
        );
        for alpha in [255, 170, 0] {
            effect.update(1, &mut random);
            assert_eq!(effect.heads[0].trails[0].alpha, alpha);
        }
        assert!(
            effect.heads[0]
                .trails
                .iter()
                .all(|trail| !trail.active && trail.age == 0)
        );
        effect.update(50, &mut random);
        effect.update(1, &mut random);
        assert_eq!(effect.heads[0].phase, 2);
        assert_eq!(effect.heads[0].phase_time, 53);
        let geometry: Vec<_> = effect.heads[0]
            .trails
            .iter()
            .map(|trail| (trail.scale, trail.angle, trail.centered))
            .collect();
        effect.update(0, &mut random);
        let head = &effect.heads[0];
        assert_eq!((head.phase, head.age, head.phase_time), (0, 0, 53));
        for (trail, (scale, angle, centered)) in head.trails.iter().zip(geometry).skip(1) {
            assert_eq!(
                (trail.scale, trail.angle, trail.centered),
                (scale, angle, centered)
            );
            assert_eq!(trail.position, head.position);
            assert!(!trail.active);
        }
        assert_eq!(head.trails[0].alpha, 255);

        host.parameters[5] = 0f32.to_bits();
        assert!(Concentration::new(&host, [64., 128.], &mut random, activation.clone()).is_err());
        host.reset_quads(0, [64., 128.])?;
        let mut empty = Concentration::new(&host, [64., 128.], &mut random, activation.clone())?;
        activation.store(20, Ordering::Relaxed);
        let mut expected = random.clone();
        empty.update(0, &mut random);
        expected.next_integer();
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(activation.load(Ordering::Relaxed), 0);
        Ok(())
    }
}
