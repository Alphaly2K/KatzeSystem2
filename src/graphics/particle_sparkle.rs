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

struct Path {
    points: [[f32; 3]; 2],
    tangent: [f32; 3],
    length: f32,
    duration: f32,
}

impl Path {
    fn new(points: [[f32; 3]; 2], duration: f32) -> Self {
        let delta = std::array::from_fn(|i| points[1][i] - points[0][i]);
        let length = length(delta);
        let tangent = if duration == 0. {
            [0.; 3]
        } else {
            delta.map(|n| (n / length) * 1.)
        };
        Self {
            points,
            tangent,
            length,
            duration,
        }
    }

    fn at(&self, mut time: f32) -> [f32; 3] {
        if time <= 0. {
            return self.points[0];
        }
        if time >= self.duration {
            time = self.duration;
        }
        let distance = (time / self.duration) * self.length;
        if self.length <= distance || distance.is_nan() {
            return self.points[1];
        }
        let t = (distance - 0.) / self.length;
        let square = t * t;
        let cube = square * t;
        let basis = [
            [2., -2., 1., 1.],
            [-3., 3., -2., -1.],
            [0., 0., 1., 0.],
            [1., 0., 0., 0.],
        ];
        let coefficients = basis.map(|row| {
            std::array::from_fn::<_, 3, _>(|i| {
                ((self.points[0][i] * row[0] + self.points[1][i] * row[1])
                    + self.tangent[i] * row[2])
                    + self.tangent[i] * row[3]
            })
        });
        std::array::from_fn(|i| {
            ((coefficients[0][i] * cube + coefficients[1][i] * square) + coefficients[2][i] * t)
                + coefficients[3][i]
        })
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    scale: f32,
    path: Path,
    position: [f32; 3],
    current_scale: [f32; 2],
    alpha: u32,
    secondary_age: u32,
    secondary_ready: bool,
    secondary_position: [f32; 3],
    secondary_scale: [f32; 2],
    secondary_alpha: u32,
}

pub(super) struct Sparkle {
    anchor: [f32; 3],
    scale: f32,
    radius: f32,
    angle: f32,
    lives: [i32; 3],
    center: [f32; 2],
    offset: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Sparkle {
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
            offset: [0.; 2],
            image,
            visible: 1,
            particles: Vec::with_capacity(host.quads.len() / 2),
        };
        for _ in 0..host.quads.len() / 2 {
            effect.particles.push(effect.spawn(random));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let scale =
            (random.next_float() as f64 * self.scale as f64 + (self.scale * 0.01) as f64) as f32;
        let sign = if random.next_integer().is_multiple_of(2) {
            1.
        } else {
            -1.
        };
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
            path: Path::new([self.anchor, endpoint], duration),
            position: self.anchor,
            current_scale: [scale; 2],
            alpha: 0,
            secondary_age: 0,
            secondary_ready: false,
            secondary_position: [self.anchor[0], self.anchor[1], self.anchor[2] + 1.],
            secondary_scale: [1.; 2],
            secondary_alpha: 0,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        self.offset = std::array::from_fn(|i| source[i] - self.center[i]);
        if random.next_integer().is_multiple_of(2) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        for i in 0..self.visible as usize {
            if self.particles[i].phase > 2 {
                self.particles[i] = self.spawn(random);
            } else {
                let p = &mut self.particles[i];
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
                        (p.scale as f64 + wave as f64 / 20.) as f32,
                        (p.scale as f64 - wave as f64 / 40.) as f32,
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
            let p = &mut self.particles[i];
            p.secondary_ready = true;
            p.secondary_position[..2].copy_from_slice(&p.position[..2]);
            p.secondary_scale = p.current_scale;
            p.secondary_alpha = if p.phase == 1 { p.alpha } else { p.alpha >> 1 };
            p.secondary_age = p.secondary_age.wrapping_add(1);
            for (axis, offset) in self.offset.into_iter().enumerate() {
                p.position[axis] += offset;
                p.secondary_position[axis] += offset;
            }
            p.age = p.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (index, p) in self.particles.iter().enumerate() {
            for secondary in [false, true] {
                let at = index + if secondary { self.particles.len() } else { 0 };
                let quad = &mut host.quads[at];
                let ready = !secondary || p.secondary_ready;
                let scale = if secondary {
                    p.secondary_scale
                } else {
                    p.current_scale
                };
                let position = if secondary {
                    p.secondary_position
                } else {
                    p.position
                };
                let alpha = if secondary {
                    p.secondary_alpha
                } else {
                    p.alpha
                };
                let mut matrix = [0.; 16];
                matrix[0] = scale[0];
                matrix[5] = scale[1];
                matrix[10] = 1.;
                matrix[15] = 1.;
                matrix[12..15].copy_from_slice(&position);
                for (i, vertex) in quad.iter_mut().enumerate() {
                    let point = transform_vector(
                        [
                            (i & 1) as f32 * self.image[0]
                                - if ready { self.image[0] * 0.5 } else { 0. },
                            (i >> 1) as f32 * self.image[1]
                                - if ready { self.image[1] * 0.5 } else { 0. },
                            0.,
                            1.,
                        ],
                        matrix,
                    );
                    vertex.position.copy_from_slice(&point[..3]);
                    vertex.rhw = point[3];
                    vertex.color = 0xffffff | (alpha << 24);
                    if ready {
                        vertex.uv = [
                            if secondary { 0.5 } else { 0. } + (i & 1) as f32 * 0.5,
                            if secondary { 0. } else { 0.5 } + (i >> 1) as f32 * 0.5,
                        ];
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn sparkle_keeps_group_activation_respawns_atlas_and_secondary_opacity() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(8, [64., 128.]).unwrap();
        for (i, value) in [607f32, 101., 21., 140., 50., 180., 10., 30.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Sparkle::new(&host, [64., 128.], &mut random);
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
                        181,
                        0,
                        0,
                        0x3e00_7f00,
                        0x4434_842d,
                        0x4302_32fa,
                        0x3e00_7f00,
                        0x3e00_7f00,
                        0,
                        2,
                    ],
                    [5, 16],
                )),
                7 => Some((
                    [
                        186,
                        181,
                        1,
                        0x3e00_7f00,
                        0x443a_320f,
                        0x430a_0e48,
                        0x3e29_74c3,
                        0x3dd8_083d,
                        127,
                        4,
                    ],
                    [11, 16],
                )),
                12 => Some((
                    [
                        197,
                        194,
                        2,
                        0x3e00_7f00,
                        0x4436_6afd,
                        0x42ca_15c0,
                        0x3df3_56bf,
                        0x3e03_e8d0,
                        238,
                        4,
                    ],
                    [16, 16],
                )),
                40 => Some((
                    [
                        193,
                        192,
                        2,
                        0x3e3a_1d34,
                        0x4443_02d8,
                        0x42e4_a390,
                        0x3e4e_9815,
                        0x3e2f_dfc3,
                        255,
                        4,
                    ],
                    [48, 32],
                )),
                159 => Some((
                    [
                        181,
                        0,
                        0,
                        0x3e09_3354,
                        0x4417_c000,
                        0x42ac_0000,
                        0x3e09_3354,
                        0x3e09_3354,
                        0,
                        4,
                    ],
                    [183, 96],
                )),
                _ => None,
            };
            if let Some((expected, calls)) = expected {
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
                        effect.visible
                    ],
                    expected,
                    "step {step}"
                );
                let mut reference = ScriptRandom::seeded(17);
                for _ in 0..calls[0] {
                    reference.next_integer();
                }
                for _ in 0..calls[1] {
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
                let view = Projection::default();
                let main = mesh.projected_quad(0, &view).unwrap();
                let secondary = mesh.projected_quad(4, &view).unwrap();
                assert_eq!(
                    [
                        main[0].position[0].to_bits(),
                        main[0].position[1].to_bits(),
                        secondary[0].position[0].to_bits(),
                        secondary[0].position[1].to_bits()
                    ],
                    [0x4441_65a8, 0x42ce_a798, 0x4441_6e37, 0x42ce_240c]
                );
                assert_eq!(
                    [main[0].color, secondary[0].color],
                    [0xffff_ffff, 0x7fff_ffff]
                );
                assert_eq!(
                    [main[0].uv, main[3].uv, secondary[0].uv, secondary[3].uv],
                    [[0., 0.5], [0.5, 1.], [0.5, 0.], [1., 0.5]]
                );
            }
        }

        for count in [0, 1] {
            host.reset_quads(count, [64., 128.]).unwrap();
            let original = host.quads.clone();
            let mut random = ScriptRandom::seeded(17);
            let mut empty = Sparkle::new(&host, [64., 128.], &mut random);
            empty.update(0, [650., 380.], &mut random);
            empty.write_mesh(&mut host);
            assert_eq!(empty.visible, 0);
            assert_eq!(host.quads, original);
            let mut reference = ScriptRandom::seeded(17);
            reference.next_integer();
            assert_eq!(random.next_integer(), reference.next_integer());
            assert_eq!(
                random.next_float().to_bits(),
                reference.next_float().to_bits()
            );
        }
    }
}
