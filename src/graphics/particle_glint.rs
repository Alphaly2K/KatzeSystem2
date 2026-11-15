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

fn radians(value: f32) -> f32 {
    (value as f64 * std::f32::consts::PI as f64 / 180.) as f32
}

fn interpolate(from: [f32; 3], to: [f32; 3], mut time: f32, duration: f32) -> [f32; 3] {
    if time < 1. {
        time = 1.;
    }
    if time as u32 as f32 >= duration {
        return to;
    }
    let cosine = |angle: f32| (angle as f64).cos() as f32;
    let sine = |angle: f32| (angle as f64).sin() as f32;
    let x = cosine(fade(radians(90.), radians(0.), duration, time)) - cosine(radians(90.));
    let y = sine(fade(radians(180.), radians(270.), duration, time)) - sine(radians(180.));
    [
        (x * (to[0] - from[0]) + from[0]) + 0.,
        (from[1] - y * (to[1] - from[1])) + 0.,
        (x * (to[2] - from[2]) + from[2]) + 0.,
    ]
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    start: [f32; 3],
    end: [f32; 3],
    position: [f32; 3],
    scale: f32,
    alpha: u32,
}

pub(super) struct Glint {
    anchor: [f32; 3],
    scale: f32,
    lives: [i32; 5],
    center: [f32; 2],
    offset: [f32; 2],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Glint {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let lives = std::array::from_fn(|i| integer(p[i + 4]));
        Self {
            anchor: [p[0], p[1], 0.],
            scale,
            lives,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            offset: [0.; 2],
            image,
            particles: (0..host.quads.len())
                .map(|i| Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    start: [0.; 3],
                    end: [0.; 3],
                    position: [p[0], p[1], 0.],
                    scale: if i == 0 { scale * 0.1 } else { 1. },
                    alpha: 0,
                })
                .collect(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        self.offset = std::array::from_fn(|i| source[i] - self.center[i]);
        let Some((head, children)) = self.particles.split_first_mut() else {
            return;
        };
        let elapsed = head.age.wrapping_sub(head.phase_time);
        head.position = self.anchor;
        match head.phase {
            0 => {
                head.scale = fade(
                    self.scale * 0.1,
                    self.scale * 1.1,
                    self.lives[0] as f32,
                    elapsed as f32,
                );
                head.alpha =
                    integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32 & 255;
                if elapsed > self.lives[0] {
                    head.phase = 1;
                    head.phase_time = head.age;
                }
            }
            1 => {
                head.scale = fade(
                    self.scale * 1.1,
                    self.scale,
                    self.lives[1] as f32,
                    elapsed as f32,
                );
                if elapsed > self.lives[1] {
                    head.phase = 2;
                    head.phase_time = head.age;
                }
            }
            2 => {
                if elapsed > self.lives[2] {
                    head.phase = 3;
                    head.phase_time = head.age;
                }
            }
            3 | 4 => {
                let (from, to, duration, next) = if head.phase == 3 {
                    (self.scale, self.scale * 1.05, self.lives[3], 4)
                } else {
                    (self.scale * 1.05, self.scale, self.lives[4], 2)
                };
                head.scale = fade(from, to, self.lives[1] as f32, elapsed as f32);
                if elapsed > duration {
                    head.phase = next;
                    head.phase_time = head.age;
                }
            }
            _ => {}
        }
        for child in children.iter_mut() {
            if child.phase != 0 || !random.next_integer().is_multiple_of(300) {
                continue;
            }
            let mut vector = [random.next_float(), random.next_float(), 0.];
            let squares = vector.map(|n| n * n);
            let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
            if length > 0. {
                vector = vector.map(|n| n / length);
            }
            for n in &mut vector[..2] {
                *n *= if random.next_integer() & 1 == 0 {
                    -1.
                } else {
                    1.
                };
            }
            let radius = (random.next_float() as f64 * 30. + 30.) as f32;
            child.start =
                std::array::from_fn(|i| (vector[i] * radius) * head.scale + self.anchor[i]);
            let radius = (random.next_float() as f64 * 30. + 30.) as f32;
            let displacement = vector.map(|n| (n * radius) * head.scale);
            child.end = std::array::from_fn(|i| child.start[i] + displacement[i]);
            child.position = head.position;
            child.scale = self.scale * 0.1;
            child.phase = 1;
            break;
        }
        let duration = children.len() as f32;
        let count = children.len() as i32;
        let elapsed = integer(delta as f32);
        for child in children.iter_mut() {
            if child.phase == 0 {
                continue;
            }
            let point = interpolate(child.start, child.end, child.age as f32, duration);
            child.position[..2].copy_from_slice(&point[..2]);
            child.alpha = integer(fade(255., 0., duration, child.age as f32)) as u32 & 255;
            child.age = child.age.wrapping_add(elapsed);
            if child.age >= count {
                child.age = 0;
                child.phase = 0;
                child.alpha = 0;
            }
            for (i, offset) in self.offset.into_iter().enumerate() {
                child.position[i] += offset;
            }
        }
        for (i, offset) in self.offset.into_iter().enumerate() {
            head.position[i] += offset;
        }
        head.age = head.age.wrapping_add(elapsed);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (quad, particle) in host.quads.iter_mut().zip(&self.particles) {
            let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
            matrix[0] = particle.scale;
            matrix[5] = particle.scale;
            matrix[12..15].copy_from_slice(&particle.position);
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
                vertex.color = 0xffffff | (particle.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glint_keeps_spawn_scans_zero_step_motion_and_pulse_boundaries() -> crate::Result<()> {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(12, [64., 128.])?;
        for (i, value) in [494f32, 100., 0., 48., 8., 5., 20., 5., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        let mut effect = Glint::new(&host, [64., 128.]);
        effect.update(1, [627., 375.], &mut random);
        for _ in 0..3 {
            expected.next_integer();
        }
        for _ in 0..4 {
            expected.next_float();
        }
        assert_eq!(
            random.clone().next_integer(),
            expected.clone().next_integer()
        );
        assert_eq!(random.clone().next_float(), expected.clone().next_float());
        let child = &effect.particles[1];
        assert_eq!((child.age, child.phase, child.alpha), (1, 1, 255));
        assert_eq!(child.start.map(f32::to_bits), [1140332738, 1120329829, 0]);
        assert_eq!(child.end.map(f32::to_bits), [1140378859, 1120282583, 0]);
        assert_eq!(child.position, [483.39374, 114.38697, 0.]);
        assert!(
            effect.particles[2..]
                .iter()
                .all(|p| p.phase == 0 && p.position == [494., 100., 0.])
        );
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color, 0xffffff);
        assert_eq!(host.quads[1][0].color, u32::MAX);
        assert_eq!(
            [host.quads[1][0].uv, host.quads[1][3].uv],
            [[0., 0.], [1., 1.]]
        );
        effect.update(0, [630.25, 367.5], &mut random);
        for _ in 0..10 {
            expected.next_integer();
        }
        assert_eq!(random.clone().next_integer(), expected.next_integer());
        assert_eq!(random.clone().next_float(), expected.next_float());
        assert_eq!(effect.particles[1].age, 1);
        assert_eq!(effect.particles[1].position, [486.64374, 106.88697, 0.]);
        assert_eq!(effect.particles[1].alpha, 231);
        effect.particles[1].age = 10;
        effect.update(1, [640., 360.], &mut random);
        assert_eq!(
            (
                effect.particles[1].age,
                effect.particles[1].phase,
                effect.particles[1].alpha
            ),
            (0, 0, 0)
        );

        host.reset_quads(1, [64., 128.])?;
        host.parameters[5] = 7f32.to_bits();
        let mut effect = Glint::new(&host, [64., 128.]);
        let mut expected = random.clone();
        effect.particles[0].phase = 3;
        effect.particles[0].age = 5;
        effect.update(0, [640., 360.], &mut random);
        assert_eq!(effect.particles[0].phase, 3);
        assert!((effect.particles[0].scale - 0.49714285).abs() < 0.0000001);
        effect.update(1, [640., 360.], &mut random);
        effect.update(0, [640., 360.], &mut random);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (4, 6)
        );
        effect.particles[0].age = 11;
        effect.update(0, [640., 360.], &mut random);
        assert_eq!(effect.particles[0].phase, 4);
        effect.update(1, [640., 360.], &mut random);
        effect.update(0, [640., 360.], &mut random);
        assert_eq!(effect.particles[0].phase, 2);
        host.reset_quads(0, [64., 128.])?;
        let mut empty = Glint::new(&host, [64., 128.]);
        empty.update(0, [0.; 2], &mut random);
        assert_eq!(empty.offset, [-640., -360.]);
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }
}
