use super::Transform;
use crate::kcs::{
    ParticleHost, ScriptRandom, ieee_float_operation, native_float_math, normalize_vector,
    transform_vector,
};

fn arithmetic(op: u8, a: f32, b: f32) -> f32 {
    f32::from_bits(ieee_float_operation(op, a.to_bits(), b.to_bits()))
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fade(from: f32, to: f32, duration: f32, time: f32) -> f32 {
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
    height: f32,
    radius: f32,
    angle: f32,
    velocity: f32,
    alpha: i32,
    lifetime: i32,
    position: [f32; 3],
    rotation: f32,
    scale: f32,
    color: u32,
    region: u32,
}

pub(super) struct DigiRing {
    anchor: [f32; 2],
    target: [f32; 2],
    emitter: [f32; 2],
    duration: i32,
    age: f32,
    scale: [f32; 2],
    height: f32,
    radius: f32,
    lifetime: i32,
    alpha: i32,
    center: [f32; 2],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl DigiRing {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            anchor: [p[0], p[1]],
            target: [p[2], p[3]],
            emitter: [p[0], p[1]],
            duration: integer(p[4]),
            age: 0.,
            scale: [p[7] / 1000., p[8] / 1000.],
            height: p[10],
            radius: p[11],
            lifetime: integer(p[12]),
            alpha: integer(p[13]),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in &host.quads {
            effect.particles.push(effect.spawn(random, [0.; 3], 0.));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom, position: [f32; 3], rotation: f32) -> Particle {
        let region = match random.next_integer() % 10 {
            0 => 0,
            1 => 1,
            2 => 3,
            _ => 2,
        };
        let scale = ((random.next_float() * 2.) - 1.) * self.scale[1] + self.scale[0];
        let channels = std::array::from_fn::<_, 3, _>(|_| random.next_integer() % 31 + 225);
        let color = channels[0] | (channels[1] << 8) | (channels[2] << 16);
        let degrees = (random.next_float() as f64 * 360.) as f32;
        let radians = (degrees as f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let angle = integer((radians as f64 * 1000.) as f32) as f32 / 1000.;
        let direction = (random.next_float() as f64 * 2. - 1.) as f32;
        let velocity = (random.next_float() as f64 * 0.1f32 as f64 * direction as f64) as f32;
        let radius = ((random.next_float() * 2.) - 1.) * 50. + self.radius;
        let height = ((random.next_float() * 2.) - 1.) * self.height + self.height;
        let alpha = self.alpha as f32;
        let alpha = ((random.next_float() * 2.) - 1.) * alpha + alpha;
        let alpha = if alpha < 0. {
            0.
        } else if alpha < 255. {
            alpha
        } else {
            255.
        };
        let lifetime = self.lifetime as f32;
        let lifetime = ((random.next_float() * 2.) - 1.) * lifetime + lifetime;
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            height,
            radius,
            angle,
            velocity,
            alpha: integer(alpha),
            lifetime: integer(lifetime),
            position,
            rotation,
            scale,
            color,
            region,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        let elapsed = delta as f32;
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        self.emitter = std::array::from_fn(|i| {
            fade(
                self.anchor[i],
                self.target[i],
                self.duration as f32,
                self.age,
            )
        });
        for i in 0..self.particles.len() {
            if self.particles[i].phase != 0 {
                if (self.duration as f32) > self.age {
                    let p = &self.particles[i];
                    self.particles[i] = self.spawn(random, p.position, p.rotation);
                }
            } else {
                let p = &mut self.particles[i];
                let age = p.age.wrapping_sub(p.phase_time);
                p.angle += p.velocity;
                let sine = f32::from_bits(native_float_math(6, &[p.angle.to_bits()]));
                let cosine = f32::from_bits(native_float_math(7, &[p.angle.to_bits()]));
                p.position = [
                    (sine as f64 * (-p.radius) as f64 + self.emitter[0] as f64) as f32,
                    p.height + self.emitter[1],
                    (cosine as f64 * p.radius as f64) as f32,
                ];
                let normal =
                    normalize_vector([self.emitter[0] - p.position[0], 0., 0. - p.position[2], 0.]);
                // 退化法线须保留 IEEE 运算的 NaN 符号与载荷。
                let dot = arithmetic(
                    2,
                    arithmetic(
                        1,
                        arithmetic(3, normal[1], 0.),
                        arithmetic(3, normal[0], 0.),
                    ),
                    arithmetic(3, normal[2], 1.),
                );
                p.rotation = f32::from_bits(native_float_math(10, &[dot.to_bits()]));
                if (normal[1] * 0. - normal[0]) + normal[2] * 0. < 0. {
                    p.rotation *= -1.;
                }
                let alpha =
                    integer(fade(p.alpha as f32, 0., p.lifetime as f32, age as f32)) as u32 & 255;
                p.color = (p.color & 0xffffff) | (alpha << 24);
                if age > p.lifetime {
                    p.phase = 3;
                    p.phase_time = p.age;
                }
            }
            let p = &mut self.particles[i];
            for (axis, value) in offset.into_iter().enumerate() {
                p.position[axis] += value;
            }
            p.age = p.age.wrapping_add(integer(elapsed));
        }
        self.age += elapsed;
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (quad, p) in host.quads.iter_mut().zip(&self.particles) {
            let region = [[0., 0.], [0.5, 0.], [0., 0.5], [0.5, 0.5]][p.region as usize];
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., p.rotation, 0.]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, v) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (i >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = p.color;
                v.uv = [
                    region[0] + (i & 1) as f32 * 0.5,
                    region[1] + (i >> 1) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(parameters: &[f32]) -> crate::Result<ParticleHost> {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.])?;
        for (i, value) in parameters.iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        Ok(host)
    }

    #[test]
    fn ring_keeps_birth_random_order_and_rotates_on_zero_time_steps() -> crate::Result<()> {
        let mut host = host(&[
            640., 730., 640., -180., 47., 10., 10., 50., 50., 250., 30., 350., 20., 200., 1.,
        ])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = DigiRing::new(&host, [64., 128.], &mut random);
        let p = &effect.particles[0];
        assert_eq!(
            [
                p.angle.to_bits(),
                p.velocity.to_bits(),
                p.radius.to_bits(),
                p.height.to_bits()
            ],
            [0x3f71eb85, 0x39081ae8, 0x4396b8b6, 0x419a36ab]
        );
        assert_eq!(
            (p.scale.to_bits(), p.color, p.alpha, p.lifetime, p.region),
            (0x3d70a892, 0xf7e4fd, 255, 38, 0)
        );
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..4 {
            expected.next_integer();
        }
        for _ in 0..8 {
            expected.next_float();
        }
        for (angle, rotation) in [(0x3f71f407, 0xbf71f406), (0x3f71fc89, 0xbf71fc89)] {
            effect.update(0, [0.; 2], &mut random);
            let p = &effect.particles[0];
            assert_eq!((p.angle.to_bits(), p.rotation.to_bits()), (angle, rotation));
            assert_eq!((p.age, p.phase, p.color), (0, 0, 0xfff7e4fd));
            assert_eq!(effect.age, 0.);
        }
        assert_eq!(
            effect.particles[0].position.map(f32::to_bits),
            [0xc3745d50, 0x43c2a36a, 0x43308133]
        );
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        effect.write_mesh(&mut host);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.], [0.5, 0.5]]
        );
        assert_ne!(host.quads[0][0].position[2], host.quads[0][1].position[2]);
        Ok(())
    }

    #[test]
    fn ring_respawns_transparently_and_stops_at_the_emitter_deadline() -> crate::Result<()> {
        let host = host(&[
            0., 0., 10., -20., 3., 0., 0., 1000., 0., 0., 2., 100., 0., 128., 0.,
        ])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = DigiRing::new(&host, [64., 128.], &mut random);
        let source = [641., 362.];
        effect.update(1, source, &mut random);
        assert_eq!(effect.particles[0].phase, 0);
        effect.update(1, source, &mut random);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (3, 1)
        );
        let previous = effect.particles[0].position;
        let rotation = effect.particles[0].rotation;
        effect.update(0, source, &mut random);
        let p = &effect.particles[0];
        assert_eq!((p.phase, p.age, p.color >> 24), (0, 0, 0));
        assert_eq!(p.rotation, rotation);
        assert_eq!(
            p.position,
            [previous[0] + 1., previous[1] + 2., previous[2]]
        );
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..8 {
            expected.next_integer();
        }
        for _ in 0..16 {
            expected.next_float();
        }
        effect.update(4, source, &mut random);
        effect.update(0, source, &mut random);
        assert_eq!(effect.particles[0].phase, 3);
        assert_eq!(effect.emitter, [10., -20.]);
        let previous = effect.particles[0].position;
        effect.update(0, source, &mut random);
        assert_eq!((effect.particles[0].phase, effect.particles[0].age), (3, 4));
        assert_eq!(
            effect.particles[0].position,
            [previous[0] + 1., previous[1] + 2., previous[2]]
        );
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }

    #[test]
    fn collapsed_and_underflowing_rings_keep_negative_nan_orientation() -> crate::Result<()> {
        let mut host = host(&[
            640., 730., 640., -180., 47., 10., 10., 50., 50., 250., 30., 350., 20., 200., 1.,
        ])?;
        for radius in [0., 1.0e-20, -1.0e-20] {
            let mut effect = DigiRing::new(&host, [64., 128.], &mut ScriptRandom::seeded(17));
            effect.particles[0].radius = radius;
            effect.update(0, [0.; 2], &mut ScriptRandom::seeded(42));
            assert_eq!(effect.particles[0].rotation.to_bits(), 0xffc0_0000);
            effect.write_mesh(&mut host);
            assert!(
                host.quads[0]
                    .iter()
                    .all(|v| v.position.iter().all(|n| n.is_nan()))
            );
        }
        Ok(())
    }
}
