use crate::kcs::ParticleHost;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    FadeIn,
    FadeOut,
    Finished,
}

#[derive(Clone, Copy, Default)]
struct Particle {
    age: i32,
    phase: Phase,
    alpha: u8,
    transformed: bool,
}

pub(super) struct SceneMagic {
    initial: [[f32; 3]; 4],
    tilted: [[f32; 3]; 4],
    particles: Vec<Particle>,
    module_state: Arc<AtomicU32>,
}

fn fade(from: f32, to: f32, duration: f32, age: f32) -> u8 {
    (if age <= 0. {
        from
    } else if age >= duration {
        to
    } else {
        (age / duration) * (to - from) + from
    }) as u8
}

impl SceneMagic {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], module_state: Arc<AtomicU32>) -> Self {
        let [x, y, scale, ..] = host.parameters.map(f32::from_bits);
        let scale = (f64::from(scale) / 100.) as f32;
        let sin_cos = |radians: f32| {
            let quantized = ((f64::from(radians) * 1000.).trunc() / 1000.) as f32;
            let (sin, cos) = f64::from(quantized).sin_cos();
            (sin as f32, cos as f32)
        };
        let (sx, cx) = sin_cos(0.15);
        let (sy, cy) = sin_cos(-0.75);
        let across = [cy, sy * sx, -sy * cx].map(|n| n * scale);
        let down = [0., cx, sx].map(|n| n * scale);
        let corners = std::array::from_fn(|i| {
            let u = (i & 1) as f32 - 0.5;
            let v = (i >> 1) as f32 - 0.5;
            [image[0] * u, image[1] * v]
        });
        let initial = corners.map(|[u, v]| [u + x, v + y, 0.]);
        let anchor = [x, y, 0.];
        let tilted = corners.map(|[u, v]| {
            std::array::from_fn(|i| {
                (f64::from(u) * f64::from(across[i])
                    + f64::from(v) * f64::from(down[i])
                    + f64::from(anchor[i])) as f32
            })
        });
        Self {
            initial,
            tilted,
            particles: vec![Particle::default(); host.quads.len()],
            module_state,
        }
    }

    pub(super) fn update(&mut self, delta: u32) {
        let count = self.particles.len() as u32;
        let next = |previous: u32| previous.wrapping_add(1).min(count);
        let previous = self
            .module_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(next(n)))
            .unwrap();
        let delta = delta as f32;
        let delta = if delta < 2147483648.0 {
            delta as i32
        } else {
            i32::MIN
        };
        for particle in &mut self.particles[..next(previous) as usize] {
            match particle.phase {
                Phase::FadeIn => {
                    particle.transformed = true;
                    particle.alpha = fade(0., 255., 80., particle.age as f32);
                    if particle.age > 80 {
                        particle.phase = Phase::FadeOut;
                    }
                }
                Phase::FadeOut => {
                    // This phase still measures from birth, so the first fade-out update hides it.
                    particle.alpha = fade(255., 0., 20., particle.age as f32);
                    if particle.age > 120 {
                        particle.phase = Phase::Finished;
                    }
                }
                Phase::Finished => {}
            }
            particle.age = particle.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let corners = if particle.transformed {
                self.tilted
            } else {
                self.initial
            };
            for (i, vertex) in quad.iter_mut().enumerate() {
                vertex.position = corners[i];
                vertex.rhw = 1.;
                vertex.color = (u32::from(particle.alpha) << 24) | 0xff_ffff;
                vertex.uv = [0.5 + (i & 1) as f32 * 0.25, 0.5 + (i >> 1) as f32 * 0.25];
            }
        }
    }
}
