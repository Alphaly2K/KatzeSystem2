use crate::kcs::{ParticleHost, ScriptRandom, ieee_float_operation, transform_vector};

fn arithmetic(op: u8, left: f32, right: f32) -> f32 {
    f32::from_bits(ieee_float_operation(op, left.to_bits(), right.to_bits()))
}

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

fn rotate(vector: [f32; 3], angle: f32) -> [f32; 3] {
    let reduced = (integer(angle * 1000.) as f32 / 1000.) as f64;
    let cosine = reduced.cos() as f32;
    let sine = reduced.sin() as f32;
    let matrix = [
        cosine, sine, 0., 0., -sine, cosine, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    // 路径方向保留标量乘加顺序，与绘图顶点的向量归约分开。
    std::array::from_fn(|i| {
        ((matrix[i] * vector[0] + matrix[i + 4] * vector[1]) + matrix[i + 8] * vector[2])
            + matrix[i + 12]
    })
}

struct Path {
    points: Vec<[f32; 3]>,
    tangents: Vec<[f32; 3]>,
    lengths: Vec<f32>,
    total: f32,
    duration: f32,
}

impl Path {
    fn new(points: Vec<[f32; 3]>, duration: f32, strength: f32) -> Self {
        let lengths: Vec<_> = std::iter::once(0.)
            .chain(
                points
                    .windows(2)
                    .map(|p| length(std::array::from_fn(|j| p[1][j] - p[0][j]))),
            )
            .collect();
        let total = lengths.iter().copied().fold(0., |a, b| a + b);
        let tangents = (0..points.len())
            .map(|i| {
                if duration == 0. {
                    [0.; 3]
                } else {
                    let delta = if i == 0 {
                        std::array::from_fn(|j| points[1][j] - points[0][j])
                    } else if i + 1 == points.len() {
                        std::array::from_fn(|j| points[i][j] - points[i - 1][j])
                    } else {
                        std::array::from_fn(|j| {
                            (points[i + 1][j] - points[i][j]) - (points[i - 1][j] - points[i][j])
                        })
                    };
                    let n = length(delta);
                    delta.map(|v| arithmetic(3, arithmetic(4, v, n), strength))
                }
            })
            .collect();
        Self {
            points,
            tangents,
            lengths,
            total,
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
        let distance = (time / self.duration) * self.total;
        let mut previous = 0.;
        for i in 0..self.points.len() - 1 {
            let next = previous + self.lengths[i + 1];
            if next > distance {
                let t = (distance - previous) / self.lengths[i + 1];
                let square = t * t;
                let cube = square * t;
                let basis = [
                    [2., -2., 1., 1.],
                    [-3., 3., -2., -1.],
                    [0., 0., 1., 0.],
                    [1., 0., 0., 0.],
                ];
                let c = basis.map(|r| {
                    std::array::from_fn::<_, 3, _>(|j| {
                        ((self.points[i][j] * r[0] + self.points[i + 1][j] * r[1])
                            + self.tangents[i][j] * r[2])
                            + self.tangents[i + 1][j] * r[3]
                    })
                });
                return std::array::from_fn(|j| {
                    ((c[0][j] * cube + c[1][j] * square) + c[2][j] * t) + c[3][j]
                });
            }
            previous = next;
        }
        *self.points.last().unwrap()
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    position: [f32; 3],
    scale: f32,
    alpha: u32,
    alpha_max: f32,
    region: usize,
}

pub(super) struct Shy {
    speed: [f32; 2],
    heading: [f32; 2],
    scale: [f32; 2],
    alpha: [f32; 2],
    lives: [i32; 3],
    rate: u32,
    batch: u32,
    base: Path,
    axis: [f32; 3],
    center: [f32; 2],
    image: [f32; 2],
    visible: u32,
    particles: Vec<Particle>,
}

impl Shy {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let lower = |v: f32, n: f32| if v < n { n } else { v };
        let clamp = |v: f32| v.clamp(-std::f32::consts::PI, std::f32::consts::PI);
        let from = [p[11], p[12], 0.];
        let to = [p[13], p[14], 0.];
        let mut delta = std::array::from_fn(|i| from[i] - to[i]);
        if length(delta) == 0. {
            delta = [0., -1., 0.];
        }
        let mut axis = rotate(delta, std::f32::consts::FRAC_PI_2);
        let n = length(axis);
        if n > 0. {
            axis = axis.map(|v| v / n);
        }
        let mut out = Self {
            speed: [lower(p[0], 10.), lower(p[1], 0.)],
            heading: [clamp(p[2] / 100.), clamp(p[3] / 100.)],
            scale: [lower(p[4] / 100., 0.01), lower(p[5] / 100., 0.)],
            alpha: [lower(p[6], 0.), lower(p[7], 0.)],
            lives: std::array::from_fn(|i| integer(p[i + 8]).max(0)),
            rate: 61i32.wrapping_sub(integer(p[15])).clamp(1, 60) as u32,
            batch: integer(p[16]).max(1) as u32,
            base: Path::new(vec![from, to], 100., 1.),
            axis,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 1,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in &host.quads {
            out.particles.push(out.spawn(random));
        }
        out
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let region = (random.next_integer() & 3) as usize;
        let scale = (random.next_float() * self.scale[1] - self.scale[1] * 0.5) + self.scale[0];
        let start = self.base.at(random.next_float() * 100.);
        let mut points = vec![start];
        let angle = (random.next_float() * std::f32::consts::PI - std::f32::consts::FRAC_PI_2)
            + self.heading[0];
        let distance = (random.next_float() * self.speed[1] + self.speed[1] * 0.5) + self.speed[0];
        let direction = rotate(self.axis, angle);
        let mut current = std::array::from_fn(|j| direction[j] * distance + start[j]);
        points.push(current);
        for step in [10, 9, 8] {
            if random.next_integer().is_multiple_of(5) {
                continue;
            }
            let angle =
                (random.next_float() * self.heading[1] - self.heading[1] * 0.5) + self.heading[0];
            let distance =
                (random.next_float() * self.speed[1] + self.speed[1] * 0.5) + self.speed[0];
            let distance = (distance as f64 * (step as f64 * 0.09)) as f32;
            let direction = rotate(self.axis, angle);
            current = std::array::from_fn(|j| current[j] + direction[j] * distance);
            points.push(current);
        }
        let duration = self.lives[2]
            .wrapping_add(self.lives[1])
            .wrapping_add(self.lives[0]) as f32;
        let alpha_max = (random.next_float() * self.alpha[1] - self.alpha[1] * 0.5) + self.alpha[0];
        Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            path: Path::new(points, duration, duration * 0.4),
            position: start,
            scale,
            alpha: 0,
            alpha_max,
            region,
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.rate) {
            self.visible = self.visible.wrapping_add(self.batch);
        }
        self.visible = self.visible.min(self.particles.len() as u32);
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let elapsed = integer(delta as f32);
        for p in &mut self.particles[..self.visible as usize] {
            let time = p.age.wrapping_sub(p.phase_time);
            if p.phase <= 2 {
                p.alpha = match p.phase {
                    0 => {
                        integer(fade(0., p.alpha_max, self.lives[0] as f32, p.age as f32)) as u32
                            & 255
                    }
                    1 => integer(p.alpha_max) as u32 & 255,
                    _ => {
                        integer(fade(p.alpha_max, 0., self.lives[2] as f32, time as f32)) as u32
                            & 255
                    }
                };
                p.position = p.path.at(p.age as f32);
                p.position[1] -= p.age as f32 * 0.1;
                let complete = match p.phase {
                    0 => p.age >= self.lives[0],
                    1 => time >= self.lives[1],
                    _ => time > self.lives[2],
                };
                if complete {
                    p.phase += 1;
                    p.phase_time = p.age;
                }
            }
            for (i, n) in offset.into_iter().enumerate() {
                p.position[i] += n;
            }
            p.age = p.age.wrapping_add(elapsed);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
            matrix[0] = p.scale;
            matrix[5] = p.scale;
            matrix[12..15].copy_from_slice(&p.position);
            let region = [[0., 0.], [0.5, 0.], [0., 0.5], [0.5, 0.5]][p.region];
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
                v.color = 0xffffff | (p.alpha << 24);
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

    fn host(count: usize, parameters: &[f32]) -> ParticleHost {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(count, [64., 128.]).unwrap();
        for (i, n) in parameters.iter().enumerate() {
            host.parameters[i] = n.to_bits();
        }
        host
    }

    #[test]
    fn paths_fade_at_integer_boundaries_and_keep_terminal_motion_and_randomness() {
        use crate::graphics::{ParticleMesh, Projection};

        let mut host = host(
            1,
            &[
                10., 30., 0., 90., 15., 10., 20., 10., 15., 100., 100., 612., 165., 652., 165.,
                60., 60., 0.,
            ],
        );
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Shy::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.particles[0].path.points.len(), 4);
        assert_eq!(effect.particles[0].scale.to_bits(), 0x3e22_908b);
        assert_eq!(effect.particles[0].alpha_max.to_bits(), 0x4199_8e78);
        for step in 1u32..160 {
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
                1 => [1, 0, 0, 0x4416_6229, 0x4334_0000, 0],
                12 => [17, 15, 1, 0x4417_8779, 0x4308_5185, 19],
                95 => [143, 115, 2, 0x4419_84f8, 0x429f_2425, 14],
                159 => [241, 222, 3, 0x4416_9152, 0x421b_8db3, 0],
                _ => continue,
            };
            let p = &effect.particles[0];
            assert_eq!(
                [
                    p.age as u32,
                    p.phase_time as u32,
                    p.phase,
                    p.position[0].to_bits(),
                    p.position[1].to_bits(),
                    p.alpha
                ],
                expected
            );
            if step != 95 && step != 159 {
                continue;
            }
            effect.write_mesh(&mut host);
            host.depth_offset = 5000.;
            let quad = ParticleMesh::new(&host, None, 59000.)
                .unwrap()
                .projected_quad(0, &Projection::default())
                .unwrap();
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for v in quad {
                for word in [
                    v.position[0].to_bits(),
                    v.position[1].to_bits(),
                    v.position[2].to_bits(),
                    v.rhw.to_bits(),
                    v.color,
                    v.uv[0].to_bits(),
                    v.uv[1].to_bits(),
                ] {
                    for byte in word.to_le_bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
            assert_eq!(
                hash,
                if step == 95 {
                    0xe285_2db8_46dc_3d79
                } else {
                    0x30df_f971_b850_b5bd
                }
            );
        }
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..163 {
            expected.next_integer();
        }
        for _ in 0..9 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
    }

    #[test]
    fn empty_groups_consume_update_randomness_and_zero_duration_paths_keep_the_fallback_axis() {
        let empty = host(0, &[]);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Shy::new(&empty, [64., 128.], &mut random);
        effect.update(0, [640., 360.], &mut random);
        assert_eq!(effect.visible, 0);
        let mut expected = ScriptRandom::seeded(17);
        expected.next_integer();
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());

        let host = host(1, &[]);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Shy::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.axis.map(f32::to_bits), [0x3f7f_fffb, 0xba50_bd10, 0]);
        assert!(
            effect.base.tangents[0]
                .iter()
                .all(|n| n.to_bits() == 0xffc0_0000)
        );
        assert!(
            effect.particles[0]
                .path
                .tangents
                .iter()
                .all(|v| *v == [0.; 3])
        );
        for (delta, phase) in [(0, 1), (0, 2), (0, 2), (1, 2), (0, 3), (100, 3)] {
            effect.update(delta, [640., 360.], &mut random);
            assert_eq!(effect.particles[0].phase, phase);
            assert_eq!(effect.particles[0].alpha, 0);
            assert!(effect.particles[0].position.iter().all(|v| v.is_finite()));
        }
    }
}
