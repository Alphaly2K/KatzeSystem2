use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

#[repr(u32)]
#[derive(Clone, Copy)]
enum Phase {
    Launch,
    FadeIn,
    Hold,
    FadeOut,
    Done,
}

struct Sparkle {
    age: i32,
    phase_start: i32,
    phase: Phase,
    velocity: [f32; 3],
    spin: f32,
    position: [f32; 3],
    angle: f32,
    alpha: u8,
}

impl Sparkle {
    fn spawn(anchor: [f32; 2], speed: f32, phase_start: i32, random: &mut ScriptRandom) -> Self {
        let z = (f64::from(random.next_float()) * 50.) as f32;
        let sign = if random.next_integer() & 1 != 0 {
            1.
        } else {
            -1.
        };
        let spin_step = f64::from(0.02f32);
        let spin = (sign * (f64::from(random.next_float()) * spin_step + spin_step)) as f32;
        let mut direction = [0.; 2];
        for _ in 0..10 {
            let [x, y] = [random.next_float(), random.next_float()].map(|v| v - 0.5);
            let length =
                ((f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32).sqrt();
            direction = if length > 0. {
                [x / length, y / length]
            } else {
                [x, y]
            };
            if direction[1] < 0.5 {
                break;
            }
        }
        Self {
            age: 0,
            phase_start,
            phase: Phase::Launch,
            velocity: [direction[0] * speed, direction[1] * speed, 0.],
            spin,
            position: [anchor[0], anchor[1], z],
            angle: 0.,
            alpha: 0,
        }
    }
}

pub(super) struct Happy {
    particles: Vec<Sparkle>,
    active: usize,
    anchor: [f32; 2],
    image: [f32; 2],
    scale: f32,
    speed: f32,
    launch: f32,
    deadlines: [f32; 3],
}

fn fade(from: f32, to: f32, start: f32, end: f32, age: f32) -> u8 {
    if age <= start {
        from as u8
    } else if age >= end {
        to as u8
    } else {
        ((age - start) / (end - start) * (to - from) + from) as u8
    }
}

impl Happy {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let [x, y, scale, speed, launch, fade_in, hold, fade_out, ..] =
            host.parameters.map(f32::from_bits);
        let anchor = [x, y];
        let speed = (f64::from(speed) / 100.) as f32;
        Self {
            particles: (0..host.quads.len())
                .map(|_| Sparkle::spawn(anchor, speed, 0, random))
                .collect(),
            active: 0,
            anchor,
            image,
            scale: (f64::from(scale) / 100.) as f32,
            speed,
            launch,
            deadlines: [fade_in, hold, fade_out],
        }
    }

    pub(super) fn update(&mut self, delta: u32, x: f32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(15) {
            self.active = (self.active + 1).min(self.particles.len());
        }
        let delta = if (delta as f32) < 2147483648. {
            (delta as f32) as i32
        } else {
            i32::MIN
        };
        let [fade_in, hold, fade_out] = self.deadlines;
        for particle in &mut self.particles[..self.active] {
            let age = particle.age as f32;
            let phase_start = particle.phase_start as f32;
            match particle.phase {
                Phase::Launch => {
                    for (p, v) in particle.position[..2].iter_mut().zip(particle.velocity) {
                        *p = (f64::from(*p) + f64::from(v) * f64::from(self.launch)) as f32;
                    }
                    particle.phase = Phase::FadeIn;
                }
                Phase::FadeIn | Phase::Hold | Phase::FadeOut => {
                    for (p, v) in particle.position.iter_mut().zip(particle.velocity) {
                        *p += v;
                    }
                    particle.angle += particle.spin;
                    let transition = match particle.phase {
                        Phase::FadeIn => {
                            particle.alpha = fade(0., 255., 0., fade_in, age);
                            (fade_in, Phase::Hold)
                        }
                        Phase::Hold => (hold, Phase::FadeOut),
                        Phase::FadeOut => {
                            particle.alpha = fade(255., 0., phase_start, fade_out, age);
                            (fade_out, Phase::Done)
                        }
                        _ => unreachable!(),
                    };
                    if f64::from(particle.age) > f64::from(transition.0) {
                        particle.phase = transition.1;
                        particle.phase_start = particle.age;
                        if matches!(particle.phase, Phase::Done) {
                            particle.alpha = 0;
                        }
                    }
                }
                Phase::Done if x >= -9000. || x.is_nan() => {
                    *particle =
                        Sparkle::spawn(self.anchor, self.speed, particle.phase_start, random);
                }
                Phase::Done => {}
            }
            particle.age = particle.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [w, h] = self.image;
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([self.scale, self.scale, 1.]);
            transform.set_rotation([0., 0., particle.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&particle.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let x = (i & 1) as f32 - 0.5;
                let y = (i >> 1) as f32 - 0.5;
                let point = transform_vector([x * w, y * h, 0., 1.], matrix);
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0x00ff_ffeb | (u32::from(particle.alpha) << 24);
            }
        }
    }
}
