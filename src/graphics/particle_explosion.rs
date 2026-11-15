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

fn linear(from: f32, to: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    base_scale: f32,
    region: u32,
    position: [f32; 3],
    angle: f32,
    scale: f32,
    alpha: u8,
}

pub(super) struct Explosion {
    image: [f32; 2],
    parameters: [f32; 8],
    module_state: Arc<AtomicU32>,
    particles: Vec<Particle>,
}

impl Explosion {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Self {
        let mut parameters = [0.; 8];
        for (to, from) in parameters.iter_mut().zip(host.parameters) {
            *to = f32::from_bits(from);
        }
        parameters[3] = (parameters[3] as f64 / 100.) as f32;
        for time in &mut parameters[4..7] {
            if *time < 1. {
                *time = 1.;
            }
        }
        parameters[7] =
            ((parameters[5] as f64 + parameters[4] as f64) + parameters[6] as f64) as f32;
        let particles = (0..host.quads.len())
            .map(|_| {
                let scale = (random.next_float() as f64 * parameters[3] as f64
                    + parameters[3] as f64) as f32;
                let position = std::array::from_fn(|axis| {
                    if axis == 2 {
                        0.
                    } else {
                        ((random.next_float() as f64 * parameters[2] as f64
                            - parameters[2] as f64 * 0.5)
                            + parameters[axis] as f64) as f32
                    }
                });
                Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    base_scale: scale,
                    region: 0,
                    position,
                    angle: 0.,
                    scale,
                    alpha: 0,
                }
            })
            .collect();
        Self {
            image,
            parameters,
            module_state,
            particles,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let count = self.particles.len() as u32;
        let before = self
            .module_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.wrapping_add(10).min(count))
            })
            .unwrap();
        let active = before.wrapping_add(10).min(count) as usize;
        let delta = integer(delta as f32);
        for p in &mut self.particles[..active] {
            if p.phase <= 2 {
                let elapsed = p.age.wrapping_sub(p.phase_time) as f32;
                let duration = self.parameters[p.phase as usize + 4];
                if p.phase == 2 {
                    p.position[1] = (p.position[1] as f64
                        - random.next_float() as f64 * 300. / duration as f64)
                        as f32;
                    p.angle = (random.next_float() as f64 * 0.3f32 as f64 + p.angle as f64) as f32;
                    p.scale = linear(
                        p.base_scale,
                        (p.base_scale as f64 / 10.) as f32,
                        duration,
                        elapsed,
                    );
                    p.alpha = integer(linear(255., 0., duration, elapsed)) as u8;
                } else {
                    p.position[1] = (p.position[1] as f64
                        - (5. / duration as f64) * random.next_float() as f64)
                        as f32;
                    if p.phase == 0 {
                        p.alpha = integer(linear(0., 255., duration, p.age as f32)) as u8;
                    }
                }
                let frame = linear(0., 24., self.parameters[7], p.age as f32);
                p.region = (frame as f64 + 0.5).floor() as u32;
                if elapsed > duration {
                    p.phase = if p.phase == 2 { 10 } else { p.phase + 1 };
                    p.phase_time = p.age;
                    if p.phase == 10 {
                        p.alpha = 0;
                    }
                }
            }
            p.age = p.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 0;
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., 0., p.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            let uv = [(p.region % 5) as f32 * 0.2, (p.region / 5) as f32 * 0.2];
            for (j, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (j & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (j >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | (u32::from(p.alpha) << 24);
                vertex.uv = [uv[0] + (j & 1) as f32 * 0.2, uv[1] + (j >> 1) as f32 * 0.2];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_counts_updates_and_fades_keep_strict_boundaries_and_terminal_randomness() {
        let mut host = ParticleHost::default();
        host.reset_quads(12, [64., 128.]).unwrap();
        for (i, n) in [640f32, 300., 100., 50., -1., 2., 3.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let state = Arc::new(AtomicU32::new(1));
        let mut effect = Explosion::new(&host, [64., 128.], &mut random, state.clone());
        assert_eq!(&effect.parameters[4..], &[1., 2., 3., 6.]);
        let last = effect.particles[11].position;
        effect.update(0, &mut random);
        assert_eq!(state.load(Ordering::Relaxed), 11);
        assert_eq!(effect.particles[11].position, last);
        assert!(
            effect.particles[..11]
                .iter()
                .all(|p| p.age == 0 && p.alpha == 0)
        );
        effect.update(1, &mut random);
        assert_eq!(state.load(Ordering::Relaxed), 12);
        assert!(effect.particles[11].position[1] < last[1]);
        effect.update(1, &mut random);
        assert!(
            effect
                .particles
                .iter()
                .all(|p| p.phase == 0 && p.alpha == 255)
        );
        effect.update(0, &mut random);
        assert!(
            effect
                .particles
                .iter()
                .all(|p| p.phase == 1 && p.phase_time == 2)
        );
        for delta in [2, 0, 1] {
            effect.update(delta, &mut random);
            assert!(effect.particles.iter().all(|p| p.phase == 1));
        }
        effect.update(0, &mut random);
        assert!(
            effect
                .particles
                .iter()
                .all(|p| p.phase == 2 && p.phase_time == 5)
        );
        effect.update(3, &mut random);
        effect.update(0, &mut random);
        assert!(effect.particles.iter().all(|p| {
            p.phase == 2 && p.alpha == 0 && p.scale == (p.base_scale as f64 / 10.) as f32
        }));
        effect.update(1, &mut random);
        effect.update(0, &mut random);
        assert!(
            effect
                .particles
                .iter()
                .all(|p| p.phase == 10 && p.region == 24)
        );
        let mut expected = random.clone();
        effect.update(100, &mut random);
        assert_eq!(random.next_float(), expected.next_float());
        assert_eq!(
            random.next_integer(),
            ScriptRandom::seeded(17).next_integer()
        );
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 0);
        assert!(
            host.quads
                .iter()
                .all(|q| q[0].uv == [0.8, 0.8] && q[0].color >> 24 == 0)
        );
    }
}
