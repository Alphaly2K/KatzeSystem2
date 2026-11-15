use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

fn integer(n: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&(n as f64)) {
        n as i32
    } else {
        i32::MIN
    }
}

fn length(v: [f32; 3]) -> f32 {
    let q = v.map(|n| n * n);
    (((q[0] + q[1]) + q[2]) as f64).sqrt() as f32
}

struct Sprite {
    origin: [f32; 3],
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    alpha: u8,
    region: Option<u32>,
}

impl Sprite {
    fn initial() -> Self {
        Self {
            origin: [0.; 3],
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
            alpha: 255,
            region: None,
        }
    }
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
    previous: [f32; 3],
    speed: f32,
    distance: f32,
    start: [f32; 3],
    velocity: [f32; 3],
    end: [f32; 3],
    position: [f32; 3],
    sprite: Sprite,
    trails: Vec<Trail>,
}

struct Config {
    image: [f32; 2],
    scale: f32,
    speed: f32,
    angle: f32,
    extent: f32,
    spread: f32,
    alpha: u8,
}

pub(super) struct Dash {
    config: Config,
    module_state: Arc<AtomicU32>,
    particles: Vec<Particle>,
}

impl Dash {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = integer(p[0]) as u32 as usize;
        let count = host
            .quads
            .len()
            .checked_div(group)
            .unwrap_or(host.quads.len());
        let mut effect = Self {
            config: Config {
                image,
                scale: p[2] / 100.,
                speed: p[1],
                angle: p[3],
                extent: p[4],
                spread: p[5],
                alpha: integer(p[6]) as u8,
            },
            module_state,
            particles: Vec::with_capacity(count),
        };
        for i in 0..count {
            let mut particle = Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                previous: [0.; 3],
                speed: 0.,
                distance: 0.,
                start: [0.; 3],
                velocity: [0.; 3],
                end: [0.; 3],
                position: [0.; 3],
                sprite: Sprite::initial(),
                trails: (1..group)
                    .map(|_| Trail {
                        age: 0,
                        active: false,
                        sprite: Sprite::initial(),
                    })
                    .collect(),
            };
            Self::spawn(&mut particle, i, &effect.config, random);
            effect.particles.push(particle);
        }
        effect
    }

    fn spawn(p: &mut Particle, index: usize, config: &Config, random: &mut ScriptRandom) {
        let s = &mut p.sprite;
        s.origin[..2].copy_from_slice(&config.image.map(|n| n * 0.5));
        s.region = Some(random.next_integer() & 3);
        s.scale = [
            (random.next_float() * config.scale) * 0.8 + config.scale * 0.2,
            1.,
            1.,
        ];
        s.alpha = 0;
        p.speed = config.speed;
        let mut offset = 0.;
        for i in 1..=5 {
            offset += (random.next_float() * 0.9 + 0.1) * ((i as f32 * 0.1) * config.spread);
            p.speed *= random.next_float() * 0.1 + 0.85;
            if random.next_integer().is_multiple_of(5) {
                break;
            }
        }
        let side = if index.is_multiple_of(2) {
            config.extent * 0.5 - offset
        } else {
            offset - config.extent * 0.5
        };
        let angle = (config.angle as f64 * std::f32::consts::PI as f64 / 180.) as f32;
        s.rotation[2] = angle;
        let quantized = integer(angle * 1000.) as f32 / 1000.;
        let cos = (quantized as f64).cos() as f32;
        let sin = (quantized as f64).sin() as f32;
        let rotate = |y| {
            [
                ((cos * side + (-sin) * y) + 0.) + 640.,
                ((sin * side + cos * y) + 0.) + 360.,
                ((side * 0. + y * 0.) + 0.) + 0.,
            ]
        };
        p.start = rotate(-config.extent);
        p.end = rotate(config.extent + config.image[1]);
        let delta = std::array::from_fn(|i| p.end[i] - p.start[i]);
        p.distance = length(delta);
        let unit = if p.distance > 0. {
            delta.map(|n| n / p.distance)
        } else {
            delta
        };
        p.velocity = unit.map(|n| n * p.speed);
        p.position = p.start;
        p.age = 0;
        p.phase = 0;
        p.previous = s.position;
        for t in &mut p.trails {
            t.age = 0;
            t.active = false;
            t.sprite.position = s.position;
            t.sprite.alpha = 0;
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let count = self.particles.len() as u32;
        // 激活计数属于已装载模块，重启实例继续使用同一计数。
        let before = self
            .module_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.wrapping_add(1).min(count))
            })
            .unwrap();
        let active = before.wrapping_add(1).min(count) as usize;
        let delta = integer(delta as f32) as u32;
        for (i, p) in self.particles[..active].iter_mut().enumerate() {
            if p.phase != 0 {
                Self::spawn(p, i, &self.config, random);
            } else {
                for axis in 0..2 {
                    p.position[axis] += p.velocity[axis];
                }
                p.sprite.position = p.position;
                p.sprite.alpha = self.config.alpha;
                if length(std::array::from_fn(|axis| p.position[axis] - p.start[axis])) > p.distance
                {
                    p.phase = 2;
                    p.phase_time = p.age;
                }
            }
            if p.phase == 0 {
                if let Some(t) = p.trails.iter_mut().find(|t| !t.active) {
                    let movement =
                        std::array::from_fn(|axis| p.sprite.position[axis] - p.previous[axis]);
                    let distance = length(movement);
                    if distance != 0. {
                        let unit = movement.map(|n| n / distance);
                        let mut angle = (unit[1] as f64).acos() as f32;
                        if movement[0] > 0. {
                            angle *= -1.;
                        }
                        t.sprite.rotation[2] = angle;
                        t.sprite.position = std::array::from_fn(|axis| {
                            (p.sprite.position[axis] + p.previous[axis]) * 0.5
                        });
                        t.sprite.origin = p.sprite.origin;
                        t.sprite.scale = p.sprite.scale;
                        t.sprite.scale[1] *= distance * 0.7;
                    } else {
                        t.sprite.position = p.sprite.position;
                        t.sprite.origin = p.sprite.origin;
                        t.sprite.scale = p.sprite.scale;
                    }
                    t.active = true;
                }
            }
            let count = p.trails.len() as u32;
            for t in &mut p.trails {
                if t.active {
                    t.sprite.alpha = integer(if t.age == 0 {
                        255.
                    } else if t.age >= count {
                        0.
                    } else {
                        (t.age as i32 as f32 / count as f32) * -255. + 255.
                    }) as u8;
                    t.age = t.age.wrapping_add(delta);
                    if t.age >= count {
                        t.age = 0;
                        t.active = false;
                        t.sprite.alpha = 0;
                    }
                }
            }
            p.previous = p.sprite.position;
            p.age = p.age.wrapping_add(delta as i32);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        for (i, p) in self.particles.iter().enumerate() {
            let sprites = std::iter::once((i, &p.sprite)).chain(
                p.trails
                    .iter()
                    .enumerate()
                    .map(|(j, t)| (self.particles.len() + i * p.trails.len() + j, &t.sprite)),
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
                            (j & 1) as f32 * self.config.image[0] - s.origin[0],
                            (j >> 1) as f32 * self.config.image[1] - s.origin[1],
                            -s.origin[2],
                            1.,
                        ],
                        matrix,
                    );
                    v.position.copy_from_slice(&point[..3]);
                    v.rhw = point[3];
                    v.color = 0xffffff | u32::from(s.alpha) << 24;
                    if let Some(region) = s.region {
                        v.uv = [(region + (j & 1) as u32) as f32 * 0.25, (j >> 1) as f32];
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_counts_updates_trails_count_time_and_activation_is_shared() -> crate::Result<()> {
        let mut host = ParticleHost::default();
        for (i, n) in [3f32, 200., 40., 90., 1280., 360., 50.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        host.reset_quads(7, [64., 128.])?;
        let remainder = host.quads[6];
        let state = Arc::new(AtomicU32::new(1));
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Dash::new(&host, [64., 128.], &mut random, state.clone());
        assert_eq!(state.load(Ordering::Relaxed), 1);
        assert_eq!(effect.particles[0].distance.to_bits(), 1_160_249_344);
        assert_eq!(effect.particles[0].sprite.position, [0.; 3]);
        effect.write_mesh(&mut host);
        assert!(host.quads[..6].iter().flatten().all(|v| v.color >> 24 == 0));
        assert_eq!(host.quads[6], remainder);
        let start = effect.particles[0].position;
        effect.update(0, &mut random);
        assert_ne!(effect.particles[0].position, start);
        assert_eq!(effect.particles[0].age, 0);
        assert_eq!(effect.particles[0].sprite.alpha, 50);
        assert_eq!(effect.particles[0].trails[0].age, 0);
        assert!(effect.particles[0].trails[0].active);
        effect.update(0, &mut random);
        assert!(effect.particles[0].trails.iter().all(|t| t.active));
        effect.update(3, &mut random);
        assert!(
            effect.particles[0]
                .trails
                .iter()
                .all(|t| !t.active && t.age == 0 && t.sprite.alpha == 0)
        );
        for _ in 0..64 {
            if effect.particles[0].phase == 2 {
                break;
            }
            effect.update(0, &mut random);
        }
        assert_eq!(effect.particles[0].phase, 2);
        let previous = effect.particles[0].sprite.position;
        effect.update(0, &mut random);
        assert_eq!(effect.particles[0].phase, 0);
        assert_eq!(effect.particles[0].sprite.position, previous);
        assert_eq!(effect.particles[0].sprite.alpha, 0);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[6], remainder);
        host.reset_quads(13, [64., 128.])?;
        let mut second = Dash::new(&host, [64., 128.], &mut random, state.clone());
        assert_eq!(state.load(Ordering::Relaxed), 2);
        second.update(0, &mut random);
        assert_eq!(state.load(Ordering::Relaxed), 3);
        assert_eq!(second.particles[2].sprite.alpha, 50);
        assert_eq!(second.particles[3].sprite.alpha, 0);
        effect.update(0, &mut random);
        assert_eq!(state.load(Ordering::Relaxed), 2);
        Ok(())
    }
}
