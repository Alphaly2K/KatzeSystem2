use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f64) -> i32 {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&value) {
        value as i64 as i32
    } else {
        0
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    spin: f32,
    path: Path,
    position: [f32; 3],
    rotation: f32,
    scale: f32,
    region: usize,
    alpha: u8,
}

pub(super) struct Rubble {
    image: [f32; 2],
    scale: f32,
    life: f32,
    increment: i32,
    visible: u32,
    base: Path,
    particles: Vec<Particle>,
}

impl Rubble {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let parameters = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            image,
            scale: (parameters[6] as f64 / 100.) as f32,
            life: parameters[7],
            increment: integer(parameters[8] as f64),
            visible: 0,
            base: Path::new_extended(
                vec![
                    parameters[..3].try_into().unwrap(),
                    parameters[3..6].try_into().unwrap(),
                ],
                100.,
                1.,
            ),
            particles: Vec::with_capacity(host.quads.len()),
        };
        for i in 0..host.quads.len() {
            let region = (random.next_integer() & 3) as usize;
            let half = effect.scale as f64 * 0.5;
            let scale = (random.next_float() as f64 * half + half) as f32;
            let time = ((100. / host.quads.len() as f64) * i as f64) as f32;
            let position = effect.base.at(time);
            let sign = if random.next_integer() & 1 != 0 {
                1.
            } else {
                -1.
            };
            let spin = ((random.next_float() as f64 * 0.019999999552965164 + 0.019999999552965164)
                * sign) as f32;
            let x = (random.next_float() as f64 * 200. + 100. + position[0] as f64) as f32;
            let y = (position[1] as f64 - (random.next_float() as f64 * 600. + 600.)) as f32;
            let sign = if random.next_integer() & 1 != 0 {
                1.
            } else {
                -1.
            };
            let z = ((random.next_float() as f64 * 50. + 50.) * sign + position[2] as f64) as f32;
            let half = effect.life as f64 * 0.5;
            let duration = (random.next_float() as f64 * half + half) as f32;
            effect.particles.push(Particle {
                age: 0,
                phase_time: 0,
                phase: 0,
                spin,
                path: Path::new_extended(
                    vec![position, [x, y, z]],
                    duration,
                    (duration as f64 * 0.550000011920929) as f32,
                ),
                position,
                rotation: 0.,
                scale,
                region,
                alpha: 0,
            });
        }
        effect
    }

    pub(super) fn update(&mut self, delta: u32) {
        self.visible = self
            .visible
            .wrapping_add(self.increment as u32)
            .min(self.particles.len() as u32);
        let delta = integer(delta as f32 as f64);
        for particle in &mut self.particles[..self.visible as usize] {
            if particle.phase <= 1 {
                particle.position = particle.path.at(particle.age as f32);
                if particle.phase == 0 {
                    particle.rotation = (particle.rotation as f64 + particle.spin as f64) as f32;
                    let duration = (particle.path.duration as f64 / 10.) as f32;
                    let time = particle.age as f32;
                    let alpha = if time <= 0. {
                        0.
                    } else if time >= duration {
                        255.
                    } else {
                        (time / duration) * 255.
                    };
                    particle.alpha = integer(alpha as f64) as u8;
                }
                if particle.age as f32 > particle.path.duration {
                    particle.phase_time = particle.age;
                    particle.phase = if particle.phase == 0 { 3 } else { 2 };
                    if particle.phase == 2 {
                        particle.alpha = 0;
                    }
                }
            }
            particle.age = particle.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([particle.scale, particle.scale, 1.]);
            transform.set_rotation([0., 0., particle.rotation]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&particle.position);
            let uv = [[0., 0.], [0.5, 0.], [0., 0.5], [0.5, 0.5]][particle.region];
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (i >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | ((particle.alpha as u32) << 24);
                vertex.uv = [uv[0] + (i & 1) as f32 * 0.5, uv[1] + (i >> 1) as f32 * 0.5];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_lifetime_keeps_the_strict_boundary_and_freezes_the_terminal_geometry() {
        let mut host = ParticleHost {
            render_mode: 3,
            ..Default::default()
        };
        host.reset_quads(3, [12., 20.]).unwrap();
        for (i, value) in [10f32, 20., 0., 50., 60., 0., 100., 0., 2.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Rubble::new(&host, [12., 20.], &mut random);
        let mut peek = random.clone();
        assert_eq!(
            (peek.next_integer(), peek.next_float().to_bits()),
            (2383608193, 1058826656)
        );
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 3);
        assert!(host.quads.iter().flatten().all(|v| v.color == 0x00ff_ffff));
        effect.update(0);
        assert_eq!(effect.visible, 2);
        effect.update(1);
        assert_eq!(effect.visible, 3);
        assert!(effect.particles.iter().all(|p| p.phase == 0));
        effect.update(0);
        assert!(
            effect
                .particles
                .iter()
                .all(|p| p.phase == 3 && p.phase_time == 1 && p.alpha == 255)
        );
        effect.write_mesh(&mut host);
        let geometry = host.quads.clone();
        effect.update(10);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads, geometry);
        assert!(effect.particles.iter().all(|p| p.age == 11));
        assert!(
            host.quads
                .iter()
                .flatten()
                .all(|v| v.position.iter().all(|n| n.is_finite()))
        );
    }
}
