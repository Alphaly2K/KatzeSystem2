use super::particle_curve::TwoPointPath;
use crate::kcs::{ParticleHost, ScriptRandom, native_triangle, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn length(vector: [f32; 3]) -> f32 {
    let squares = vector.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
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
    scale: f32,
    path: TwoPointPath,
    position: [f32; 3],
    current_scale: [f32; 2],
    alpha: u32,
}

pub(super) struct Warukira {
    anchor: [f32; 3],
    scale: f32,
    radius: f32,
    angle: f32,
    lives: [i32; 3],
    center: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Warukira {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let mut effect = Self {
            anchor: [p[0], p[1], 0.],
            scale,
            radius: p[3],
            angle: p[4],
            lives: std::array::from_fn(|i| integer(p[i + 5]).max(0)),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 1,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for index in 0..host.quads.len() {
            effect.particles.push(effect.spawn(index, random));
        }
        effect
    }
    fn spawn(&self, index: usize, random: &mut ScriptRandom) -> Particle {
        let scale = (random.next_float() * self.scale) * 0.5 + self.scale * 0.5;
        let sign = if index.is_multiple_of(2) { 1. } else { -1. };
        let endpoint = |angle: f32| {
            let radians = (angle as f64 * std::f32::consts::PI as f64 / 180.) as f32;
            let sine = (radians as f64).sin() as f32;
            let cosine = (radians as f64).cos() as f32;
            [
                (cosine as f64 * sign as f64 - sine as f64 * 0.) as f32,
                (cosine as f64 * 0. + sine as f64 * sign as f64) as f32,
            ]
        };
        let negative = endpoint(-self.angle);
        let positive = endpoint(self.angle);
        let spread = std::array::from_fn::<_, 2, _>(|i| {
            let delta = positive[i] - negative[i];
            let square = (delta as f64).powf(2.) as f32;
            (square as f64).sqrt() as f32
        });
        let x = (random.next_float() * spread[0] - spread[0] * 0.5) + sign;
        let y = (random.next_float() * spread[1] - spread[1] * 0.5) + 0.;
        let size = length([x, y, 0.]);
        let direction = if size > 0. {
            [x / size, y / size, 0. / size]
        } else {
            [x, y, 0.]
        };
        let distance =
            (random.next_float() as f64 * self.radius as f64 + self.radius as f64) as f32;
        let endpoint = std::array::from_fn(|i| self.anchor[i] + direction[i] * distance);
        let duration = self.lives.into_iter().fold(0i32, i32::wrapping_add) as f32;
        Particle {
            age: self.lives[0],
            phase_time: 0,
            phase: 0,
            scale,
            path: TwoPointPath::new([self.anchor, endpoint], duration, 1.),
            position: self.anchor,
            current_scale: [scale; 2],
            alpha: 0,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        if random.next_integer().is_multiple_of(5) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for index in 0..self.visible as usize {
            if self.particles[index].phase > 2 {
                self.particles[index] = self.spawn(index, random);
            } else {
                let p = &mut self.particles[index];
                p.position = p.path.at(p.age as f32);
                let elapsed = p.age.wrapping_sub(p.phase_time);
                if p.phase == 0 {
                    if p.age > self.lives[0] {
                        p.phase = 1;
                        p.phase_time = p.age;
                    }
                } else {
                    let wave = native_triangle(p.age as f32, 30.);
                    p.current_scale = [
                        (p.scale as f64 + wave as f64 / 5.) as f32,
                        (p.scale as f64 - wave as f64 / 10.) as f32,
                    ];
                    let (from, to) = if p.phase == 1 { (0., 255.) } else { (255., 0.) };
                    p.alpha = integer(fade(
                        from,
                        to,
                        self.lives[p.phase as usize] as f32,
                        elapsed as f32,
                    )) as u32
                        & 255;
                    if elapsed > self.lives[p.phase as usize] {
                        p.phase += 1;
                        p.phase_time = p.age;
                    }
                }
            }
            let p = &mut self.particles[index];
            for (axis, value) in offset.into_iter().enumerate() {
                p.position[axis] += value;
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
            matrix[0] = p.current_scale[0];
            matrix[5] = p.current_scale[1];
            matrix[12..15].copy_from_slice(&p.position);
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
                vertex.color = 0xffffff | (p.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn scatter_preserves_alternating_paths_fades_respawns_and_randomness() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(8, [64., 128.]).unwrap();
        for (i, value) in [671f32, 193., 54., 230., 40., 190., 20., 15.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Warukira::new(&host, [64., 128.], &mut random);
        for step in 1u32..160 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(
                delta,
                [
                    640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                    360. + ((step % 7) as i32 - 3) as f32 * -7.5,
                ],
                &mut random,
            );
            let expected = match step {
                1 => Some((
                    [
                        191, 0, 0, 0x3edb7656, 0x445a7c32, 0x43641704, 0x3edb7656, 0x3edb7656, 0, 2,
                    ],
                    32,
                )),
                7 => Some((
                    [
                        196, 191, 1, 0x3edb7656, 0x44607d29, 0x436c028d, 0x3ecdcf15, 0x3ee249f6,
                        63, 4,
                    ],
                    32,
                )),
                40 => Some((
                    [
                        204, 191, 1, 0x3ee51fe0, 0x447a2eb3, 0x438b5149, 0x3e858d1a, 0x3f0a74a2,
                        153, 8,
                    ],
                    48,
                )),
                159 => Some((
                    [
                        220, 215, 2, 0x3e9a8e98, 0x44649dc0, 0x42831119, 0x3eec7a1d, 0x3e6331ab,
                        187, 8,
                    ],
                    192,
                )),
                _ => None,
            };
            if let Some((expected, floats)) = expected {
                let p = &effect.particles[0];
                assert_eq!(
                    [
                        p.age as u32,
                        p.phase_time as u32,
                        p.phase,
                        p.scale.to_bits(),
                        p.position[0].to_bits(),
                        p.position[1].to_bits(),
                        p.current_scale[0].to_bits(),
                        p.current_scale[1].to_bits(),
                        p.alpha,
                        effect.visible,
                    ],
                    expected,
                    "step {step}"
                );
                for (index, p) in effect.particles.iter().enumerate() {
                    assert_eq!(
                        p.path.points[1][0] > p.path.points[0][0],
                        index.is_multiple_of(2)
                    );
                }
                let mut reference = ScriptRandom::seeded(17);
                for _ in 0..step {
                    reference.next_integer();
                }
                for _ in 0..floats {
                    reference.next_float();
                }
                let mut actual = random.clone();
                assert_eq!(actual.next_integer(), reference.next_integer());
                assert_eq!(
                    actual.next_float().to_bits(),
                    reference.next_float().to_bits()
                );
            }
            if step == 40 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                for (index, xy) in [[0x4478187f, 0x4374056a], [0x43854530, 0x4375725a]]
                    .into_iter()
                    .enumerate()
                {
                    let quad = mesh.projected_quad(index, &Projection::default()).unwrap();
                    assert_eq!(
                        [quad[0].position[0].to_bits(), quad[0].position[1].to_bits()],
                        xy
                    );
                    assert_eq!(quad[0].color, 0x99ffffff);
                    assert_eq!([quad[0].uv, quad[3].uv], [[0., 0.], [1., 1.]]);
                }
            }
        }
    }

    #[test]
    fn zero_radius_paths_stay_at_anchor_and_empty_groups_consume_activation_draws() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        for (i, value) in [44f32, 55., -10., 0., 0., 2., 3., 4.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        for count in [0, 1] {
            host.reset_quads(count, [64., 128.]).unwrap();
            let mut random = ScriptRandom::seeded(17);
            let mut effect = Warukira::new(&host, [64., 128.], &mut random);
            for p in &effect.particles {
                assert_eq!(p.path.tangent.map(f32::to_bits), [0xffc00000; 3]);
                assert_eq!(p.path.at(3.), [44., 55., 0.]);
                assert!(p.scale >= 0.0005 && p.scale <= 0.001);
            }
            effect.update(0, [650., 380.], &mut random);
            assert_eq!(effect.visible, count as u32);
            for p in &effect.particles {
                assert_eq!(p.position, [54., 75., 0.]);
            }
            let mut reference = ScriptRandom::seeded(17);
            reference.next_integer();
            for _ in 0..count * 4 {
                reference.next_float();
            }
            assert_eq!(random.next_integer(), reference.next_integer());
            assert_eq!(
                random.next_float().to_bits(),
                reference.next_float().to_bits()
            );
        }
    }
}
