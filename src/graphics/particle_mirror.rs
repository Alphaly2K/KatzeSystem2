use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, native_float_math, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn clamp(value: f32, maximum: f32) -> f32 {
    if value < 0. {
        0.
    } else if value < maximum {
        value
    } else {
        maximum
    }
}

fn math(index: u32, value: f32) -> f32 {
    f32::from_bits(native_float_math(index, &[value.to_bits()]))
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
    radius: f32,
    angle: f32,
    velocity: f32,
    alpha_max: i32,
    lives: [i32; 3],
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    colors: [u32; 4],
}

pub(super) struct Mirror {
    anchor: [f32; 2],
    center: [f32; 2],
    tilt: f32,
    scale: [f32; 2],
    radius: f32,
    alpha: i32,
    lives: [i32; 3],
    image: [f32; 2],
    mode: u32,
    visible: usize,
    particles: Vec<Particle>,
}

impl Mirror {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let tilt = (p[3] as f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let mut effect = Self {
            anchor: [p[0], p[1]],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            tilt: clamp(tilt, std::f32::consts::FRAC_PI_2),
            scale: [p[6] / 1000., p[7] / 1000.],
            radius: p[8],
            alpha: integer(p[9]),
            lives: std::array::from_fn(|i| integer(p[10 + i])),
            image,
            mode: if integer(p[13]) != 0 {
                1
            } else {
                host.render_mode
            },
            visible: 0,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in &host.quads {
            effect.particles.push(effect.spawn(random));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let scale = (random.next_float() * 2. - 1.) * self.scale[1] + self.scale[0];
        let mut colors = [0xffffff; 4];
        for color in &mut colors[..2] {
            let primary = random.next_integer() % 3;
            let channels: [u32; 3] = std::array::from_fn(|i| {
                if i as u32 == primary {
                    random.next_integer() % 31 + 225
                } else {
                    random.next_integer() % 30
                }
            });
            *color = channels[0] | (channels[1] << 8) | (channels[2] << 16);
        }
        let degrees = (random.next_float() as f64 * 360.) as f32;
        let angle = (degrees as f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let angle = integer(angle * 1000.) as f32 / 1000.;
        let jitter = (random.next_float() as f64 * 2. - 1.) as f32;
        let velocity = (random.next_float() as f64 * 0.01f32 as f64 * jitter as f64) as f32;
        let radius = (((random.next_float() * 2. - 1.) * self.radius) * 0.5) + self.radius;
        let alpha = self.alpha as f32;
        let alpha_max = integer(clamp((random.next_float() * 2. - 1.) * alpha + alpha, 255.));
        let lives = self.lives.map(|life| {
            let life = life as f32;
            integer(((random.next_float() * 2. - 1.) * life) * 0.5 + life)
        });
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            radius,
            angle,
            velocity,
            alpha_max,
            lives,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [scale, scale * 6., 1.],
            colors,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        self.visible = (self.visible + 1).min(self.particles.len());
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let elapsed = integer(delta as f32);
        for index in 0..self.visible {
            if self.particles[index].phase > 2 {
                let mut next = self.spawn(random);
                next.position = self.particles[index].position;
                next.rotation = self.particles[index].rotation;
                self.particles[index] = next;
            } else {
                let p = &mut self.particles[index];
                let time = p.age.wrapping_sub(p.phase_time);
                p.angle += p.velocity;
                p.position = [
                    (math(6, p.angle) as f64 * (-p.radius) as f64 + self.anchor[0] as f64) as f32,
                    self.anchor[1],
                    (math(7, p.angle) as f64 * p.radius as f64) as f32,
                ];
                let mut direction = [self.anchor[0] - p.position[0], 0., -p.position[2]];
                let squares = direction.map(|n| n * n);
                let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
                if length > 0. {
                    direction = direction.map(|n| n / length);
                }
                let cosine = (direction[1] * 0. + direction[0] * 0.) - direction[2] * 1.;
                let sign = (direction[1] * 0. - direction[0] * 1.) + direction[2] * 0.;
                let yaw = math(10, cosine);
                p.rotation = [-self.tilt, if sign < 0. { -yaw } else { yaw }, 0.];
                if p.phase != 1 {
                    let (from, to) = if p.phase == 0 {
                        (0., p.alpha_max as f32)
                    } else {
                        (p.alpha_max as f32, 0.)
                    };
                    let alpha = integer(fade(
                        from,
                        to,
                        p.lives[p.phase as usize] as f32,
                        time as f32,
                    )) as u8;
                    for color in &mut p.colors {
                        *color = (*color & 0xffffff) | (u32::from(alpha) << 24);
                    }
                }
                if time > p.lives[p.phase as usize] {
                    p.phase += 1;
                    p.phase_time = p.age;
                }
            }
            let p = &mut self.particles[index];
            for (position, offset) in p.position[..2].iter_mut().zip(offset) {
                *position += offset;
            }
            p.age = p.age.wrapping_add(elapsed);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale(p.scale);
            transform.set_rotation(p.rotation);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                        (i >> 1) as f32 * self.image[1] - self.image[1],
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = p.colors[i];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_phases_keep_colors_and_respawn_retains_geometry_before_source_offset() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [
            640f32, 360., 90., 30., 2., 1., 400., 40., 100., 120., 0., 0., 0., 1.,
        ]
        .into_iter()
        .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Mirror::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.visible, 0);
        assert_eq!(
            effect.particles[0].colors,
            [0x000d14fd, 0x00170ae8, 0x00ffffff, 0x00ffffff]
        );
        let mut peek = random.clone();
        assert_eq!(peek.next_integer(), 1_801_551_568);
        assert_eq!(peek.next_float().to_bits(), 1_057_886_970);
        for (delta, state) in [
            (0, [0, 0, 0, 0]),
            (1, [1, 0, 0, 0]),
            (0, [1, 1, 1, 77]),
            (0, [1, 1, 1, 77]),
            (1, [2, 1, 1, 77]),
            (0, [2, 2, 2, 77]),
            (0, [2, 2, 2, 77]),
            (1, [3, 2, 2, 77]),
            (0, [3, 3, 3, 0]),
            (0, [0, 0, 0, 0]),
        ] {
            effect.update(delta, [647.25, 353.], &mut random);
            let p = &effect.particles[0];
            assert_eq!(
                [
                    p.age as u32,
                    p.phase_time as u32,
                    p.phase,
                    p.colors[0] >> 24
                ],
                state
            );
            assert_eq!(p.colors[0] >> 24, p.colors[3] >> 24);
        }
        let p = &effect.particles[0];
        assert_eq!(
            p.position.map(f32::to_bits),
            [0x44193355, 0x43ad0000, 0x41f1052b]
        );
        assert_eq!(p.rotation.map(f32::to_bits), [0xbf060a92, 0xbf71f32b, 0]);
        assert_eq!(p.colors, [0x0010f40d, 0x001bf715, 0x00ffffff, 0x00ffffff]);
        assert_eq!(random.next_integer(), 3_674_283_866);
        assert_eq!(random.next_float().to_bits(), 1_058_826_656);
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert!(
            host.quads[0]
                .iter()
                .all(|v| v.position.iter().all(|n| n.is_finite()))
        );
    }
}
