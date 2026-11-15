use super::{Transform, particle_curve::Path, particle_plugin::next_c_random};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};
use std::sync::{Arc, atomic::AtomicU32};

fn integer(n: f64) -> i32 {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&n) {
        n as i64 as i32
    } else {
        0
    }
}

fn length(v: [f32; 3]) -> f32 {
    let q = v.map(|n| n as f64 * n as f64);
    ((((q[0] + q[1]) + q[2]) as f32) as f64).sqrt() as f32
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let n = length(v);
    if n > 0. { v.map(|v| v / n) } else { v }
}

fn alpha(start: f32, end: f32, time: f32) -> u32 {
    let value = if time <= start {
        255.
    } else if time >= end {
        0.
    } else {
        ((time - start) / (end - start)) * -255. + 255.
    };
    integer(value as f64) as u32 & 255
}

#[derive(Clone)]
struct Sprite {
    origin: [f32; 3],
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    color: u32,
}

struct Trail {
    age: u32,
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

pub(super) struct Paper {
    image: [f32; 2],
    duration: i32,
    scale: f32,
    frequency: u32,
    increment: i32,
    cycling: bool,
    active: i32,
    module_state: Arc<AtomicU32>,
    particles: Vec<Particle>,
}

impl Paper {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = integer(p[0] as f64) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(group)
            .unwrap_or(host.quads.len());
        let prefill = integer(p[5] as f64) != 0;
        let mut effect = Self {
            image,
            duration: integer(p[1] as f64),
            scale: (p[2] as f64 / 100.) as f32,
            frequency: 61i32.wrapping_sub(integer(p[3] as f64)).clamp(1, 60) as u32,
            increment: integer(p[4] as f64),
            cycling: integer(p[6] as f64) != 0,
            active: if prefill { count as i32 / 2 } else { 0 },
            module_state,
            particles: Vec::with_capacity(count),
        };
        for i in 0..count {
            let initial = Sprite {
                origin: [0.; 3],
                position: [0.; 3],
                rotation: [0.; 3],
                scale: [1.; 3],
                color: 0xffff_ffff,
            };
            let mut particle = Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                path: Path::new_extended(vec![[0.; 3]; 2], 0., 0.),
                previous: [0.; 3],
                direction: [0.; 3],
                sprite: initial.clone(),
                trails: (1..group)
                    .map(|_| Trail {
                        age: 0,
                        active: false,
                        sprite: initial.clone(),
                    })
                    .collect(),
            };
            Self::spawn(
                &mut particle,
                image,
                effect.scale,
                effect.duration,
                random,
                &effect.module_state,
            );
            if prefill && i < effect.active as usize {
                particle.age = integer(random.next_float() as f64 * effect.duration as f64);
            }
            effect.particles.push(particle);
        }
        effect
    }

    fn spawn(
        p: &mut Particle,
        image: [f32; 2],
        scale: f32,
        duration: i32,
        random: &mut ScriptRandom,
        module_state: &AtomicU32,
    ) {
        let s = &mut p.sprite;
        s.origin[..2].copy_from_slice(&image.map(|n| (n as f64 * 0.5) as f32));
        let size = (random.next_float() as f64 * f64::from(0.05f32) + scale as f64) as f32;
        s.scale = [size, size, 1.];
        s.color = [
            0xf0f0f0, 0xffff6e, 0xff6eff, 0x6effff, 0xff6e6e, 0x6eff6e, 0x6e6eff,
        ][(random.next_integer() % 7) as usize];
        s.position = [
            (random.next_float() as f64 * 1280.) as f32,
            -50.,
            (random.next_float() as f64 * 100.) as f32,
        ];
        s.rotation = std::array::from_fn(|_| {
            (random.next_float() as f64 * std::f32::consts::PI as f64) as f32
        });
        p.age = 0;
        p.phase = 0;
        p.previous = s.position;
        let mut points = vec![s.position];
        let initial = normalized([
            (random.next_float() as f64 - 0.5) as f32,
            random.next_float(),
            0.,
        ]);
        let distance = (f64::from(next_c_random(module_state) % 500) / 10. + 50.) as f32;
        let movement = initial.map(|n| n * distance);
        let mut point = std::array::from_fn(|i| s.position[i] + movement[i]);
        points.push(point);
        p.direction = normalized(std::array::from_fn(|i| point[i] - s.position[i]));
        let bounds = [-10., 10.].map(|angle| {
            let angle = (angle * std::f32::consts::PI as f64 / 180.) as f32;
            let cos = (angle as f64).cos() as f32;
            let sin = (angle as f64).sin() as f32;
            [
                (cos as f64 * p.direction[0] as f64 - sin as f64 * p.direction[1] as f64) as f32,
                (cos as f64 * p.direction[1] as f64 + sin as f64 * p.direction[0] as f64) as f32,
            ]
        });
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let delta = bounds[0][i] - bounds[1][i];
            ((delta as f64 * delta as f64) as f32 as f64).sqrt() as f32
        });
        for _ in 0..12 {
            let perturb =
                spread.map(|n| (random.next_float() as f64 * n as f64 - n as f64 * 0.5) as f32);
            let dir = normalized([p.direction[0] + perturb[0], p.direction[1] + perturb[1], 0.]);
            let distance = (f64::from(random.next_integer() % 500) / 10. + 50.) as f32;
            let movement = dir.map(|n| n * distance);
            point = std::array::from_fn(|i| point[i] + movement[i]);
            points.push(point);
        }
        p.path = Path::new_extended(points, duration as f32, 50.);
        for t in &mut p.trails {
            t.age = 0;
            t.active = false;
            t.sprite.position = s.position;
            t.sprite.color = s.color;
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.frequency) {
            self.active = self.active.wrapping_add(self.increment);
        }
        self.active = self.active.min(self.particles.len() as i32);
        for p in self.particles.iter_mut().take(self.active.max(0) as usize) {
            if p.phase <= 1 {
                p.sprite.position = p.path.at(p.age as f32);
                let alpha = if p.phase == 0 {
                    255
                } else {
                    alpha(p.phase_time as f32, self.duration as f32, p.age as f32)
                };
                p.sprite.color = (p.sprite.color & 0xffffff) | alpha << 24;
                for angle in &mut p.sprite.rotation {
                    *angle = (random.next_float() as f64 / 10. + *angle as f64) as f32;
                }
                if p.phase == 0 {
                    let threshold = (self.duration as f64 * f64::from(0.9f32)) as f32;
                    if p.age > integer((threshold as f64 + 0.5).floor()) {
                        p.phase = 1;
                        p.phase_time = p.age;
                    }
                } else if p.age > self.duration {
                    p.phase = 2;
                    p.phase_time = p.age;
                }
            } else if self.cycling {
                Self::spawn(
                    p,
                    self.image,
                    self.scale,
                    self.duration,
                    random,
                    &self.module_state,
                );
            }
            if p.phase == 0 {
                if let Some(t) = p.trails.iter_mut().find(|t| !t.active) {
                    let movement = std::array::from_fn(|i| p.sprite.position[i] - p.previous[i]);
                    if length(movement) != 0. {
                        t.sprite.rotation = p.sprite.rotation;
                        t.sprite.position = std::array::from_fn(|i| {
                            ((p.previous[i] as f64 + p.sprite.position[i] as f64) * 0.5) as f32
                        });
                    } else {
                        t.sprite.position = p.sprite.position;
                    }
                    t.sprite.origin = p.sprite.origin;
                    t.sprite.scale = p.sprite.scale;
                    t.active = true;
                }
            }
            let count = p.trails.len() as u32;
            for t in &mut p.trails {
                if t.active {
                    let alpha = alpha(0., count as f32, t.age as f32);
                    t.sprite.color = (t.sprite.color & 0xffffff) | alpha << 24;
                    t.age = t.age.wrapping_add(1);
                    if t.age >= count {
                        t.age = 0;
                        t.active = false;
                    }
                }
            }
            p.previous = p.sprite.position;
            p.age = p.age.wrapping_add(integer(delta as f32 as f64));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (index, p) in self.particles.iter().enumerate() {
            let sprites = std::iter::once((index, &p.sprite)).chain(
                p.trails
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (self.particles.len() + index * p.trails.len() + i, &t.sprite)),
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
                    v.color = s.color;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn prefill_preserves_curve_rounding_update_based_trails_and_rebirth_state() -> crate::Result<()>
    {
        let mut host = ParticleHost::default();
        for (i, n) in [3f32, 40., 65., 60., 2., 1., 1.].into_iter().enumerate() {
            host.parameters[i] = n.to_bits();
        }
        host.reset_quads(7, [64., 128.])?;
        let remainder = host.quads[6];
        let state = Arc::new(AtomicU32::new(1));
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Paper::new(&host, [64., 128.], &mut random, state.clone());
        let first = &effect.particles[0];
        assert_eq!(effect.active, 1);
        assert_eq!(first.age, 26);
        assert_eq!(first.path.points.len(), 14);
        assert_eq!(
            first.path.points[1].map(f32::to_bits),
            [1_129_753_630, 3_206_242_880, 1_113_911_928]
        );
        assert_eq!(first.path.total.to_bits(), 1_147_045_481);
        assert_eq!(first.sprite.color, 0xff6e6e);
        assert_eq!(first.trails[0].sprite.origin, [0.; 3]);
        assert_eq!(first.trails[0].sprite.scale, [1.; 3]);
        assert_eq!(state.load(Ordering::Relaxed), 3_357_800_067);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        assert_eq!(host.quads[6], remainder);

        effect.update(0, &mut random);
        assert_eq!(effect.active, 2);
        let first = &effect.particles[0];
        assert_eq!(first.age, 26);
        assert_eq!(first.sprite.color, 0xffff6e6e);
        assert_eq!(first.trails[0].age, 1);
        assert_eq!(first.trails[0].sprite.color >> 24, 255);
        let mut peek = random.clone();
        assert_eq!(peek.next_integer(), 2_024_897_102);
        assert_eq!(peek.next_float().to_bits(), 1_059_541_751);
        effect.update(1, &mut random);
        assert!(!effect.particles[0].trails[0].active);
        assert_eq!(effect.particles[0].trails[0].sprite.color >> 24, 127);
        for delta in [2, 4, 7] {
            effect.update(delta, &mut random);
        }
        assert_eq!(effect.particles[0].phase, 0);
        assert_eq!(effect.particles[0].age, 40);
        effect.update(11, &mut random);
        assert_eq!(effect.particles[0].phase, 1);
        assert_eq!(effect.particles[0].phase_time, 40);
        effect.update(0, &mut random);
        assert_eq!(effect.particles[0].phase, 2);
        assert_eq!(effect.particles[0].sprite.color >> 24, 0);
        effect.update(1, &mut random);
        let first = &effect.particles[0];
        assert_eq!(first.phase, 0);
        assert_eq!(first.age, 1);
        assert_eq!(first.phase_time, 51);
        assert_eq!(first.sprite.color, 0xff6eff);
        assert_eq!(first.trails[0].sprite.color, 0xffff6eff);
        assert_eq!(state.load(Ordering::Relaxed), 415_139_642);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[6], remainder);
        Ok(())
    }
}
