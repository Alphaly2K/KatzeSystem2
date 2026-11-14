use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

struct Path {
    start: [f32; 3],
    end: [f32; 3],
    length: f32,
    duration: f32,
    coefficients: [[f32; 3]; 4],
}

impl Path {
    fn new(start: [f32; 3], end: [f32; 3], duration: f32) -> Self {
        let delta = std::array::from_fn::<_, 3, _>(|i| end[i] - start[i]);
        let squares = delta.map(|n| n * n);
        let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
        let tangent = if duration == 0. {
            [0.; 3]
        } else {
            delta.map(|n| (n / length) * 10.)
        };
        let basis = [
            [2., -2., 1., 1.],
            [-3., 3., -2., -1.],
            [0., 0., 1., 0.],
            [1., 0., 0., 0.],
        ];
        let coefficients = basis.map(|row| {
            std::array::from_fn(|i| {
                let values = [start[i], end[i], tangent[i], tangent[i]];
                ((values[0] * row[0] + values[1] * row[1]) + values[2] * row[2])
                    + values[3] * row[3]
            })
        });
        Self {
            start,
            end,
            length,
            duration,
            coefficients,
        }
    }

    fn at(&self, mut time: f32) -> [f32; 3] {
        if time <= 0. {
            return self.start;
        }
        if time >= self.duration {
            time = self.duration;
        }
        // 保留按总长度再换算区间参数的逐步舍入，以及零时长的末点选择。
        let distance = (time / self.duration) * self.length;
        if self.length.partial_cmp(&distance) != Some(std::cmp::Ordering::Greater) {
            return self.end;
        }
        let t = distance / self.length;
        let square = t * t;
        let cube = square * t;
        std::array::from_fn(|i| {
            ((self.coefficients[0][i] * cube + self.coefficients[1][i] * square)
                + self.coefficients[2][i] * t)
                + self.coefficients[3][i]
        })
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: usize,
    angle: f32,
    scale: f32,
    lives: [i32; 3],
    path: Path,
    position: [f32; 3],
    alpha: u32,
}

pub(super) struct Anger {
    anchor: [f32; 2],
    scale: f32,
    lives: [i32; 3],
    factor: f32,
    center: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

impl Anger {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let params = host.parameters.map(f32::from_bits);
        let mut scale = params[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let mut effect = Self {
            anchor: [params[0], params[1]],
            scale,
            lives: std::array::from_fn(|i| integer(params[i + 3])),
            factor: if image[1] > 256. { 1. } else { 0.5 },
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 1,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for i in 0..host.quads.len() {
            let particle = effect.spawn(i, random);
            effect.particles.push(particle);
        }
        effect
    }

    fn spawn(&self, index: usize, random: &mut ScriptRandom) -> Particle {
        let scale =
            ((random.next_float() * self.scale) as f64 * 0.4 + (self.scale * 0.6) as f64) as f32;
        let lives = self
            .lives
            .map(|n| integer((random.next_float() * n as f32) * 0.5 + n as f32 * 0.5));
        let mut angle = (random.next_float() as f64 * 0.5 + 0.5) as f32;
        if index.is_multiple_of(2) {
            angle *= -1.;
        }
        let distance = ((random.next_float() * 200. + 350.) * self.scale) * self.factor;
        let x = if index.is_multiple_of(2) {
            self.anchor[0] - distance
        } else {
            distance + self.anchor[0]
        };
        let y = self.anchor[1] - ((random.next_float() * 130. + 120.) * self.scale) * self.factor;
        let start = [x, y, 0.];
        let speed = (random.next_float() * 100. + 100.) * (self.factor * self.scale);
        let direction = integer(angle * 1000.) as f32 / 1000.;
        let sine = (direction as f64).sin() as f32;
        let cosine = (direction as f64).cos() as f32;
        let velocity = [sine, -cosine, 0.];
        let end = std::array::from_fn(|i| velocity[i] * speed + start[i]);
        let path = Path::new(start, end, lives[0].wrapping_add(lives[1]) as f32);
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            angle,
            scale,
            lives,
            path,
            position: start,
            alpha: 0,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        if random.next_integer().is_multiple_of(3) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for i in 0..self.visible as usize {
            if self.particles[i].phase > 2 {
                self.particles[i] = self.spawn(i, random);
            } else {
                let particle = &mut self.particles[i];
                particle.position = particle.path.at(particle.age as f32);
                let elapsed = particle.age.wrapping_sub(particle.phase_time);
                let duration = particle.lives[particle.phase];
                if particle.phase != 1 {
                    let (from, to) = if particle.phase == 0 {
                        (0., 255.)
                    } else {
                        (255., 0.)
                    };
                    let time = elapsed as f32;
                    let alpha = if time <= 0. {
                        from
                    } else if time >= duration as f32 {
                        to
                    } else {
                        (time / duration as f32) * (to - from) + from
                    };
                    particle.alpha = integer(alpha) as u32 & 255;
                }
                if elapsed > duration {
                    particle.phase = if particle.phase == 2 {
                        10
                    } else {
                        particle.phase + 1
                    };
                    particle.phase_time = particle.age;
                }
            }
            let particle = &mut self.particles[i];
            for (axis, value) in offset.into_iter().enumerate() {
                particle.position[axis] += value;
            }
            particle.age = particle.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [width, height] = self.image;
        for (particle, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([particle.scale, particle.scale, 1.]);
            transform.set_rotation([0., 0., particle.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&particle.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (i & 1) as f32 * width - width * 0.5,
                        (i >> 1) as f32 * height - height * 0.5,
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0x00ff_ffff | (particle.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anger_preserves_curve_rounding_fades_and_respawn_random_consumption() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [901f32, 176., 86., 10., 15., 25.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(0);
        let mut effect = Anger::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.particles[0].lives, [9, 11, 20]);
        assert_eq!(effect.particles[0].path.length.to_bits(), 0x429b_e946);
        for step in 1u32..96 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            let source = [
                640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                360. + ((step % 7) as i32 - 3) as f32 * -7.5,
            ];
            effect.update(delta, source, &mut random);
            let expected = match step {
                2 => [2, 0, 0, 0x4433_a1d3, 0x42e6_c9f1, 0x1c],
                40 => [3, 0, 0, 0x442e_8e2e, 0x4298_c321, 0x3f],
                95 => [45, 28, 2, 0x442d_6f20, 0x4250_05c2, 0x28],
                _ => continue,
            };
            let particle = &effect.particles[0];
            assert_eq!(
                [
                    particle.age as u32,
                    particle.phase_time as u32,
                    particle.phase as u32,
                    particle.position[0].to_bits(),
                    particle.position[1].to_bits(),
                    particle.alpha
                ],
                expected
            );
        }
        let mut expected = ScriptRandom::seeded(0);
        for _ in 0..95 {
            expected.next_integer();
        }
        for _ in 0..24 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        let path = Path::new([10., 20., 0.], [3., 9., 0.], 0.);
        assert_eq!(path.at(0.), [10., 20., 0.]);
        assert_eq!(path.at(1.), [3., 9., 0.]);
    }
}
