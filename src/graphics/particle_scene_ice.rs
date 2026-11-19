use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f64) -> i32 {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&value) {
        value as i64 as i32
    } else {
        0
    }
}

fn normalize(direction: [f32; 2]) -> [f32; 2] {
    let [x, y] = direction.map(f64::from);
    let length = ((x * x + y * y) as f32).sqrt();
    if length > 0. {
        direction.map(|axis| axis / length)
    } else {
        direction
    }
}

fn alpha(from: f32, to: f32, duration: i32, time: i32) -> u8 {
    let (duration, time) = (duration as f32, time as f32);
    let value = if time <= 0. {
        from
    } else if time >= duration {
        to
    } else {
        (time / duration) * (to - from) + from
    };
    integer(value as f64) as u8
}

struct Particle {
    path: Path,
    age: i32,
    phase_time: i32,
    phase: u8,
    position: [f32; 3],
    scale: f32,
    spin: f32,
    rotation: f32,
    opacity: u8,
}

pub(super) struct SceneIce {
    particles: Vec<Particle>,
    image: [f32; 2],
    visible: usize,
    fade_in: i32,
    flicker_end: i32,
    fade_out: i32,
}

impl SceneIce {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let scale = (p[2] as f64 / 100.) as f32;
        let drift = (p[4] as f64 / 100.) as f32;
        let spread = (p[5] as f64 / 100.) as f32;
        let direction = integer(p[6] as f64);
        let vertical = (p[7] as f64 / 100.) as f32;
        let fade_in = integer(p[9] as f64);
        let flicker_end = integer(p[10] as f64);
        let fade_out = integer(p[11] as f64);
        let lifetime = fade_in.wrapping_add(flicker_end).wrapping_add(fade_out) as f32;
        let base = Path::new_extended(
            vec![[0., 280., 0.], [270., 470., 0.], [940., 550., 0.]],
            100.,
            200.,
        );
        let angle = (10. * std::f32::consts::PI as f64 / 180.) as f32;
        let (sin, cos) = ((angle as f64).sin() as f32, (angle as f64).cos() as f32);
        let particles = (0..host.quads.len())
            .map(|_| {
                let size = (random.next_float() as f64 * scale as f64 + scale as f64) as f32;
                let mut spin = (random.next_float() as f64 * 0.08f32 as f64) as f32;
                if random.next_integer() & 1 != 0 {
                    spin = -spin;
                }
                let anchor = base.at((random.next_float() as f64 * 100.) as f32);
                let initial = [
                    (random.next_float() as f64 * 200. - 100. + anchor[0] as f64) as f32,
                    (random.next_float() as f64 * 100. - 50. + anchor[1] as f64) as f32,
                    (random.next_float() as f64 * 50.) as f32,
                ];
                let x = (random.next_float() as f64 * spread as f64 - drift as f64) as f32;
                let y = random.next_float() as f64 * vertical as f64;
                let y = match direction {
                    1 => -(y + 0.05f32 as f64),
                    2 => y - vertical as f64 * 0.5,
                    _ => y + 0.05f32 as f64,
                } as f32;
                let [x, y] = normalize([x, y]);
                let distance = (random.next_float() as f64 * p[3] as f64 + p[3] as f64) as f32;
                let mut point = [
                    initial[0] + x * distance,
                    initial[1] + y * distance,
                    initial[2],
                ];
                let mut points = vec![initial, point];
                let [x, y] = normalize([point[0] - initial[0], point[1] - initial[1]]);
                let low = [
                    (x as f64 * cos as f64 + y as f64 * sin as f64) as f32,
                    (y as f64 * cos as f64 - x as f64 * sin as f64) as f32,
                ];
                let high = [
                    (x as f64 * cos as f64 - y as f64 * sin as f64) as f32,
                    (y as f64 * cos as f64 + x as f64 * sin as f64) as f32,
                ];
                for _ in 0..4 {
                    let jitter = |axis: usize, value: f32, random: &mut ScriptRandom| {
                        let range = (high[axis] - low[axis]).abs();
                        let offset = ((random.next_float() as f64 - 0.5) * range as f64) as f32;
                        value + offset
                    };
                    let [x, y] = normalize([jitter(0, x, random), jitter(1, y, random)]);
                    let distance = (random.next_float() as f64 * p[3] as f64 + p[3] as f64) as f32;
                    point[0] += x * distance;
                    point[1] += y * distance;
                    points.push(point);
                    if random.next_integer().is_multiple_of(15) {
                        break;
                    }
                }
                Particle {
                    path: Path::new_extended(points, lifetime, p[8]),
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    position: initial,
                    scale: size,
                    spin,
                    rotation: 0.,
                    opacity: 0,
                }
            })
            .collect();
        Self {
            particles,
            image,
            visible: 2,
            fade_in,
            flicker_end,
            fade_out,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        self.visible = (self.visible + 10).min(self.particles.len());
        let delta = integer(delta as f32 as f64);
        for particle in &mut self.particles[..self.visible] {
            let time = particle.age.wrapping_sub(particle.phase_time);
            let limit = match particle.phase {
                0 => {
                    particle.position = particle.path.at(particle.age as f32);
                    particle.opacity = alpha(0., 255., self.fade_in, particle.age);
                    Some((particle.age, self.fade_in))
                }
                1 => {
                    particle.position = particle.path.at(particle.age as f32);
                    particle.opacity = random.next_integer() as u8;
                    Some((particle.age, self.flicker_end))
                }
                2 => {
                    particle.position = particle.path.at(particle.age as f32);
                    particle.opacity = alpha(190., 0., self.fade_out, time);
                    particle.rotation += 0.01f32;
                    Some((time, self.fade_out))
                }
                _ => None,
            };
            if let Some((age, duration)) = limit {
                particle.rotation += particle.spin;
                if age > duration {
                    particle.phase += 1;
                    particle.phase_time = particle.age;
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
            for (i, vertex) in quad.iter_mut().enumerate() {
                let [x, y] = [(i & 1) as f32, (i >> 1) as f32];
                let point = transform_vector(
                    [(x - 0.5) * self.image[0], (y - 0.5) * self.image[1], 0., 1.],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | (u32::from(particle.opacity) << 24);
                vertex.uv = [0.5 + x * 0.5, 0.5 + y * 0.5];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_out_uses_total_age_for_position_and_phase_age_for_alpha() {
        let mut host = ParticleHost::default();
        host.reset_quads(1, [128.; 2]).unwrap();
        let parameters = [
            0f32, 0., 100., 4., 150., -100., 50., 1., 50., 200., 20., 80.,
        ];
        for (slot, value) in host.parameters.iter_mut().zip(parameters) {
            *slot = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = SceneIce::new(&host, [128.; 2], &mut random);
        effect.update(281, &mut random);
        effect.update(1, &mut random);
        effect.update(80, &mut random);
        effect.update(1, &mut random);
        let last = &effect.particles[0];
        assert_eq!(last.phase, 2);
        assert_eq!(
            last.position.map(f32::to_bits),
            [0x43b4_5407, 0x43e2_0772, 0x4180_82e4]
        );
        assert_eq!(last.opacity, 0);
        effect.update(1, &mut random);
        assert_eq!(effect.particles[0].phase, 3);
    }
}
