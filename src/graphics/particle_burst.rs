use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, normalize_vector, transform_vector};
use crate::{Error, Result};

struct Particle {
    age: i32,
    phase_value: i32,
    phase: usize,
    velocity: [f32; 2],
    spin: f32,
    center: [f32; 2],
    angle: f32,
    scale: f32,
    region: [f32; 2],
    alpha: u8,
}

pub(super) struct Burst {
    image: [f32; 2],
    extent: f32,
    mode: u32,
    particles: Vec<Particle>,
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

impl Burst {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Result<Self> {
        let [x, y, scale, speed, alpha, regions, ..] = host.parameters.map(f32::from_bits);
        let regions = integer(f64::from(regions)) as u32;
        let count = host.quads.len();
        if regions > 4 && count != 0 {
            return Err(Error::new(0, "粒子图集分区数量超过上限"));
        }
        let spacing = (360. / count as f64) as f32;
        let particles = (0..count)
            .map(|index| {
                let region = if regions == 0 {
                    0
                } else {
                    random.next_integer() % regions
                };
                let scale =
                    (f64::from(scale) / 100. * (1. + f64::from(random.next_float()))) as f32;
                let angle =
                    (f64::from(random.next_float()) * f64::from(std::f32::consts::PI)) as f32;
                let degrees = (f64::from(spacing) * index as f64) as f32;
                let direction =
                    (f64::from(degrees) * f64::from(std::f32::consts::PI) / 180.) as f32;
                let direction = (f64::from(integer(f64::from(direction) * 100.)) / 100.) as f32;
                let (sin, cos) = f64::from(direction).sin_cos();
                let unit = normalize_vector([-(sin as f32), cos as f32, 0., 0.]);
                let magnitude = (f64::from(speed) * (0.5 + f64::from(random.next_float()))) as f32;
                let velocity = [unit[0] * magnitude, unit[1] * magnitude];
                let spin = (f64::from(random.next_float()) / 5.) as f32;
                let spin = if random.next_integer() & 1 != 0 {
                    -spin
                } else {
                    spin
                };
                Particle {
                    age: 0,
                    phase_value: 0,
                    phase: 0,
                    velocity,
                    spin,
                    center: std::array::from_fn(|i| {
                        (f64::from([x, y][i]) + f64::from(velocity[i]) * 10.) as f32
                    }),
                    angle,
                    scale,
                    region: [(region & 1) as f32 * 0.5, (region >> 1) as f32 * 0.5],
                    alpha: 0,
                }
            })
            .collect();
        Ok(Self {
            image,
            extent: if regions == 0 { 1. } else { 0.5 },
            mode: if alpha < 1. { 1 } else { host.render_mode },
            particles,
        })
    }

    pub(super) fn update(&mut self, delta: u32, parameters: &[u32; 32]) {
        let p = parameters.map(f32::from_bits);
        let increment = integer(f64::from(delta as f32));
        for particle in &mut self.particles {
            if particle.phase < 3 {
                let age = particle.age as f32;
                let (movement, spin_divisor) = match particle.phase {
                    0 => (1., 1.),
                    1 => (f64::from(0.3_f32), 5.),
                    _ => (f64::from(0.3_f32), 10.),
                };
                for (center, velocity) in particle.center.iter_mut().zip(particle.velocity) {
                    *center = (f64::from(*center) + f64::from(velocity) * movement) as f32;
                }
                particle.angle =
                    (f64::from(particle.angle) + f64::from(particle.spin) / spin_divisor) as f32;
                let alpha = if particle.phase == 2 {
                    let from = particle.phase_value as f32;
                    let fade = if age <= p[7] {
                        from
                    } else if age >= p[8] {
                        0.
                    } else {
                        ((age - p[7]) / (p[8] - p[7])) * -from + from
                    };
                    integer(f64::from(fade))
                } else {
                    integer(f64::from(p[4]) * 64. + 191.)
                };
                particle.alpha = alpha as u8;
                if f64::from(particle.age) > f64::from(p[6 + particle.phase]) {
                    particle.phase += 1;
                    if particle.phase == 3 {
                        particle.phase_value = particle.age;
                        particle.alpha = 0;
                    } else {
                        particle.phase_value = i32::from(particle.alpha);
                    }
                }
            }
            particle.age = particle.age.wrapping_add(increment);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([particle.scale, particle.scale, 1.]);
            transform.set_rotation([0., 0., particle.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..14].copy_from_slice(&particle.center);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let u = (i & 1) as f32;
                let v = (i >> 1) as f32;
                let point = transform_vector(
                    [(u - 0.5) * self.image[0], (v - 0.5) * self.image[1], 0., 1.],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.uv = [
                    particle.region[0] + u * self.extent,
                    particle.region[1] + v * self.extent,
                ];
                vertex.color = u32::from(particle.alpha) << 24 | 0xff_ffff;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_moves_when_age_is_unchanged_and_hides_after_its_last_phase() -> Result<()> {
        let mut host = ParticleHost::default();
        host.reset_quads(1, [20., 16.])?;
        for (slot, value) in host
            .parameters
            .iter_mut()
            .zip([100f32, 120., 40., 4., 0.25, 4., 1., 3., 6.])
        {
            *slot = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Burst::new(&host, [20., 16.], &mut random)?;
        effect.write_mesh(&mut host);
        let initial = host.quads[0][0];
        assert_eq!(initial.color >> 24, 0);
        effect.update(0, &host.parameters);
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert_ne!(host.quads[0][0].position, initial.position);
        assert_eq!(host.quads[0][0].color >> 24, 207);
        for delta in [2, 2, 1, 2, 0] {
            effect.update(delta, &host.parameters);
        }
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        let terminal = host.quads[0];
        effect.update(30, &host.parameters);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0], terminal);
        Ok(())
    }
}
