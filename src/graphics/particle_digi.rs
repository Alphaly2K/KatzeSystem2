use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

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
    position: [f32; 3],
    velocity: [f32; 3],
    alpha: i32,
    lifetime: f32,
    draw_position: [f32; 3],
    scale: f32,
    color: u32,
    region: u32,
}

pub(super) struct Digi {
    anchor: [f32; 2],
    target: [f32; 2],
    emitter: [f32; 2],
    duration: i32,
    age: f32,
    speed: [f32; 2],
    scale: [f32; 2],
    scatter: [f32; 2],
    lifetime: i32,
    alpha: i32,
    center: [f32; 2],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Digi {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut effect = Self {
            anchor: [p[0], p[1]],
            target: [p[2], p[3]],
            emitter: [p[0], p[1]],
            duration: integer(p[4]),
            age: 0.,
            speed: [p[5] / 100., p[6] / 100.],
            scale: [p[7] / 1000., p[8] / 1000.],
            scatter: [p[9], p[10]],
            lifetime: integer(p[11]),
            alpha: integer(p[12]),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in &host.quads {
            effect.particles.push(effect.spawn(random, [0.; 3]));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom, draw_position: [f32; 3]) -> Particle {
        let region = match random.next_integer() % 10 {
            0 => 0,
            1 => 1,
            2 => 3,
            _ => 2,
        };
        let scale = ((random.next_float() * 2.) - 1.) * self.scale[1] + self.scale[0];
        let channels = std::array::from_fn::<_, 3, _>(|_| random.next_integer() % 31 + 225);
        let color = channels[0] | (channels[1] << 8) | (channels[2] << 16);
        let position = [
            ((random.next_float() * 2.) - 1.) * self.scatter[0] + self.emitter[0],
            ((random.next_float() * 2.) - 1.) * self.scatter[1] + self.emitter[1],
            (random.next_float() as f64 * 5.) as f32,
        ];
        let mut velocity = [0.; 3];
        for value in &mut velocity[..2] {
            *value = random.next_float();
            if random.next_integer().is_multiple_of(2) {
                *value *= -1.;
            }
        }
        let squares = velocity.map(|n| n * n);
        let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
        if length > 0. {
            velocity = velocity.map(|n| n / length);
        }
        let speed = ((random.next_float() * 2.) - 1.) * self.speed[1] + self.speed[0];
        velocity = velocity.map(|n| n * speed);
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
            position,
            velocity,
            alpha: integer(alpha),
            lifetime,
            draw_position,
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
                    self.particles[i] = self.spawn(random, self.particles[i].draw_position);
                }
            } else {
                let p = &mut self.particles[i];
                let time = p.age.wrapping_sub(p.phase_time) as f32;
                for axis in 0..2 {
                    // 速度按更新次数消费，时间步只推进年龄。
                    p.position[axis] += p.velocity[axis];
                    p.draw_position[axis] = p.position[axis];
                }
                let alpha = integer(fade(p.alpha as f32, 0., p.lifetime, time)) as u32 & 255;
                p.color = (p.color & 0xffffff) | (alpha << 24);
                if time > p.lifetime {
                    p.phase = 3;
                    p.phase_time = p.age;
                }
            }
            let p = &mut self.particles[i];
            for (axis, value) in offset.into_iter().enumerate() {
                p.draw_position[axis] += value;
            }
            p.age = p.age.wrapping_add(integer(elapsed));
        }
        self.age += elapsed;
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (quad, p) in host.quads.iter_mut().zip(&self.particles) {
            let region = [[0., 0.], [0.5, 0.], [0., 0.5], [0.5, 0.5]][p.region as usize];
            let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
            matrix[0] = p.scale;
            matrix[5] = p.scale;
            matrix[12..15].copy_from_slice(&p.draw_position);
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
    fn digi_keeps_birth_random_order_and_moves_on_zero_time_steps() -> crate::Result<()> {
        let mut host = host(&[
            -40., 390., 640., 780., 90., 300., 60., 600., 120., 20., 30., 30., 128., 1.,
        ])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Digi::new(&host, [64., 128.], &mut random);
        let p = &effect.particles[0];
        assert_eq!(
            p.position.map(f32::to_bits),
            [0xc257ecc4, 0x43c52bc9, 0x3d37aa7f]
        );
        assert_eq!(p.velocity.map(f32::to_bits), [0xbe21c1d7, 0xc0611581, 0]);
        assert_eq!(
            (
                p.scale.to_bits(),
                p.color,
                p.alpha,
                p.lifetime.to_bits(),
                p.region
            ),
            (0x3f1efa91, 0xf7e4fd, 248, 0x41c956ce, 0)
        );
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..6 {
            expected.next_integer();
        }
        for _ in 0..9 {
            expected.next_float();
        }
        for position in [[0xc2588e86, 0x43c3699e], [0xc2593048, 0x43c1a773]] {
            effect.update(0, [640., 360.], &mut random);
            let p = &effect.particles[0];
            assert_eq!([p.position[0].to_bits(), p.position[1].to_bits()], position);
            assert_eq!(p.draw_position, [p.position[0], p.position[1], 0.]);
            assert_eq!((p.age, p.phase, p.color), (0, 0, 0xf8f7e4fd));
            assert_eq!(effect.age, 0.);
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        effect.write_mesh(&mut host);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.], [0.5, 0.5]]
        );
        assert!(host.quads[0].iter().all(|v| v.color == 0xf8f7e4fd));
        Ok(())
    }

    #[test]
    fn digi_respawns_after_expiry_and_stops_spawning_at_the_emitter_deadline() -> crate::Result<()>
    {
        let host = host(&[
            0., 0., 10., -20., 3., 100., 0., 1000., 0., 0., 0., 0., 128., 0.,
        ])?;
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Digi::new(&host, [64., 128.], &mut random);
        let source = [641., 362.];
        effect.update(1, source, &mut random);
        assert_eq!(effect.particles[0].phase, 0);
        effect.update(1, source, &mut random);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (3, 1)
        );
        let previous = effect.particles[0].draw_position;
        effect.update(0, source, &mut random);
        let p = &effect.particles[0];
        assert_eq!((p.phase, p.age, p.color >> 24), (0, 0, 0));
        assert_eq!(p.draw_position, [previous[0] + 1., previous[1] + 2., 0.]);
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..12 {
            expected.next_integer();
        }
        for _ in 0..18 {
            expected.next_float();
        }
        effect.update(4, source, &mut random);
        effect.update(0, source, &mut random);
        assert_eq!(effect.particles[0].phase, 3);
        assert_eq!(effect.emitter, [10., -20.]);
        let previous = effect.particles[0].draw_position;
        effect.update(0, source, &mut random);
        assert_eq!((effect.particles[0].phase, effect.particles[0].age), (3, 4));
        assert_eq!(
            effect.particles[0].draw_position,
            [previous[0] + 1., previous[1] + 2., 0.]
        );
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }
}
