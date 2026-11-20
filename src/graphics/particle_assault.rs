use super::{Transform, particle_curve::TwoPointPath};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

#[derive(Clone, Copy)]
enum Phase {
    FadeIn,
    Travel,
    FadeOut,
    Rebirth,
}

struct Particle {
    age: i32,
    phase_start: i32,
    phase: Phase,
    path: TwoPointPath,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: f32,
    region: u32,
    alpha: u8,
}

struct Config {
    start: [f32; 3],
    end: [f32; 3],
    scale: f32,
    spread: f32,
    phases: [i32; 3],
    lifetime: i32,
}

pub(super) struct Assault {
    image: [f32; 2],
    config: Config,
    particles: Vec<Particle>,
    active: usize,
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn length([x, y]: [f32; 2]) -> f32 {
    ((f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32).sqrt()
}

fn fade(a: f32, b: f32, duration: i32, age: i32) -> u8 {
    let time = age as f32;
    let duration = duration as f32;
    if time <= 0. {
        a as u8
    } else if time >= duration {
        b as u8
    } else {
        ((time / duration) * (b - a) + a) as u8
    }
}

impl Config {
    fn spawn(&self, phase_start: i32, random: &mut ScriptRandom) -> Particle {
        let region = (random.next_integer() % 30).min(3);
        let scale =
            (f64::from(random.next_float()) * f64::from(self.scale) + f64::from(self.scale)) as f32;
        let tilt = if region < 2 { [0.2, 0.9] } else { [0.; 2] };
        let angle = (f64::from(random.next_float()) * f64::from(std::f32::consts::PI)) as f32;
        let time = (f64::from(random.next_float()) * 100.) as f32;
        let offset = (time / 100.) * self.spread - self.spread * 0.5;
        let quantized = (f64::from(integer(f64::from(offset) * 1000.)) / 1000.) as f32;
        let (sin, cos) = f64::from(quantized).sin_cos();
        let (sin, cos) = (f64::from(sin as f32), f64::from(cos as f32));
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        // The rotated displacement is shifted by the origin before normalization.
        let direction = [
            (f64::from(dx) * cos - f64::from(dy) * sin) as f32 + self.start[0],
            (f64::from(dx) * sin + f64::from(dy) * cos) as f32 + self.start[1],
        ];
        let normal = length(direction);
        let direction = if normal > 0. {
            direction.map(|n| n / normal)
        } else {
            direction
        };
        let half = length([dx, dy]) * 0.5;
        let distance = (f64::from(half) * (f64::from(random.next_float()) + 1.)) as f32;
        let target = [direction[0] * distance, direction[1] * distance, 0.];
        Particle {
            age: 0,
            phase_start,
            phase: Phase::FadeIn,
            path: TwoPointPath::new_extended([self.start, target], self.lifetime as f32, 1.),
            position: self.start,
            rotation: [tilt[0], tilt[1], angle],
            scale,
            region,
            alpha: 0,
        }
    }
}

impl Assault {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let [
            x,
            y,
            end_x,
            end_y,
            scale,
            spread,
            rise,
            travel,
            fade,
            prefill,
            ..,
        ] = host.parameters.map(f32::from_bits);
        let phases = [rise, travel, fade].map(|n| integer(f64::from(n)));
        let config = Config {
            start: [x, y, 0.],
            end: [end_x, end_y, 0.],
            scale: scale / 1000.,
            spread: (f64::from(spread) * f64::from(std::f32::consts::PI) / 180.) as f32,
            phases,
            lifetime: phases.into_iter().fold(0, i32::wrapping_add),
        };
        let count = host.quads.len();
        let stagger = integer(f64::from(prefill)) != 0;
        let active = if stagger { count / 2 } else { 1 };
        let particles = (0..count)
            .map(|i| {
                let mut p = config.spawn(0, random);
                if stagger && i < active {
                    p.age = integer(f64::from(random.next_float()) * f64::from(config.lifetime));
                }
                p
            })
            .collect();
        Self {
            image,
            config,
            particles,
            active,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let [rise, travel, fade_out] = self.config.phases;
        let delta = integer(f64::from(delta as f32));
        self.active = (self.active + 1).min(self.particles.len());
        for p in &mut self.particles[..self.active] {
            let elapsed = p.age.wrapping_sub(p.phase_start);
            let next = match p.phase {
                Phase::FadeIn => {
                    p.position = p.path.at(p.age as f32);
                    p.alpha = fade(0., 255., rise, p.age);
                    (p.age > rise).then_some(Phase::Travel)
                }
                Phase::Travel => {
                    p.position = p.path.at(p.age as f32);
                    (elapsed > travel).then_some(Phase::FadeOut)
                }
                Phase::FadeOut => {
                    p.position = p.path.at(p.age as f32);
                    p.alpha = fade(255., 0., fade_out, elapsed);
                    (elapsed > fade_out).then_some(Phase::Rebirth)
                }
                Phase::Rebirth => {
                    *p = self.config.spawn(p.phase_start, random);
                    None
                }
            };
            if let Some(phase) = next {
                p.phase = phase;
                p.phase_start = p.age;
            }
            p.rotation[2] += 0.05;
            p.scale *= 0.997;
            p.age = p.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [w, h] = self.image;
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation(p.rotation);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, v) in quad.iter_mut().enumerate() {
                let [x, y] = [(i & 1) as f32, (i >> 1) as f32];
                let point = transform_vector([(x - 0.5) * w, (y - 0.5) * h, 0., 1.], matrix);
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = (v.color & 0xff_ffff) | (u32::from(p.alpha) << 24);
                v.uv = [
                    ((p.region & 1) as f32 + x) * 0.5,
                    ((p.region >> 1) as f32 + y) * 0.5,
                ];
            }
        }
    }
}
