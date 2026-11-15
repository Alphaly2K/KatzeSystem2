use crate::kcs::{ParticleHost, ScriptRandom, ieee_float_operation, transform_vector};

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
    points: [[f32; 3]; 3],
    tangents: [[f32; 3]; 3],
    lengths: [f32; 3],
    total: f32,
    duration: f32,
}

impl Path {
    fn new(points: [[f32; 3]; 3], duration: f32) -> Self {
        let deltas: [[f32; 3]; 2] =
            std::array::from_fn(|i| std::array::from_fn(|j| points[i + 1][j] - points[i][j]));
        let lengths = [0., length(deltas[0]), length(deltas[1])];
        let total = (0. + lengths[1]) + lengths[2];
        let mut tangents = [[0.; 3]; 3];
        if duration != 0. {
            let strength = total / 2.;
            let middle: [f32; 3] = std::array::from_fn(|j| {
                (points[2][j] - points[1][j]) - (points[0][j] - points[1][j])
            });
            let denominators = [length(deltas[0]), length(middle), length(deltas[1])];
            tangents = std::array::from_fn(|i| {
                [deltas[0], middle, deltas[1]][i]
                    .map(|n| arithmetic(3, arithmetic(4, n, denominators[i]), strength))
            });
        }
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
        for i in 0..2 {
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
                let c = basis.map(|row| {
                    std::array::from_fn::<_, 3, _>(|j| {
                        ((self.points[i][j] * row[0] + self.points[i + 1][j] * row[1])
                            + self.tangents[i][j] * row[2])
                            + self.tangents[i + 1][j] * row[3]
                    })
                });
                return std::array::from_fn(|j| {
                    ((c[0][j] * cube + c[1][j] * square) + c[2][j] * t) + c[3][j]
                });
            }
            previous = next;
        }
        self.points[2]
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    position: [f32; 3],
    scale: f32,
    colors: [u32; 4],
    region: u32,
}

pub(super) struct Energy {
    scale: f32,
    variation: f32,
    lives: [i32; 5],
    scatter: f32,
    anchor: [f32; 3],
    target: [f32; 3],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Energy {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let scale = [p[4], p[5]].map(|n| {
            let n = n / 100.;
            if n < 0.001 { 0.001 } else { n }
        });
        let mut effect = Self {
            scale: scale[0],
            variation: scale[1],
            lives: std::array::from_fn(|i| integer(p[i + 8]).max(0)),
            scatter: p[6],
            anchor: [p[0], p[1], 0.],
            target: [p[2], p[3], 0.],
            image,
            particles: Vec::with_capacity(host.quads.len()),
        };
        for _ in &host.quads {
            effect.particles.push(effect.spawn(random));
        }
        effect
    }

    fn spawn(&self, random: &mut ScriptRandom) -> Particle {
        let region = random.next_integer() % 3;
        let scale = ((random.next_float() * 2.) - 1.) * self.variation + self.scale;
        let xy = [random.next_float(), random.next_float()].map(|n| (n as f64 * 2. - 1.) as f32);
        let direction = [xy[0], xy[1], 0.];
        let size = length([direction[1], direction[0], direction[2]]);
        let direction = if size > 0. {
            direction.map(|n| n / size)
        } else {
            direction
        };
        let x = (random.next_float() * self.scatter + self.scatter) * direction[0];
        let y = ((random.next_float() * self.scatter) + self.scatter * 0.5) * direction[1];
        let middle = [
            self.anchor[0] + x,
            self.anchor[1] + y,
            self.anchor[2] + direction[2],
        ];
        let duration = self.lives[2]
            .wrapping_add(self.lives[1])
            .wrapping_add(self.lives[0]) as f32;
        Particle {
            age: self.lives[0],
            phase_time: 0,
            phase: 0,
            path: Path::new([self.anchor, middle, self.target], duration),
            position: self.anchor,
            scale,
            colors: [0xffffff; 4],
            region,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let elapsed = delta as f32;
        for particle in &mut self.particles {
            if particle.phase > 2 {
                continue;
            }
            let phase_time = particle.age.wrapping_sub(particle.phase_time);
            particle.position = particle.path.at(particle.age as f32);
            let alpha = match particle.phase {
                0 => 0,
                1 => integer(fade(0., 255., self.lives[1] as f32, phase_time as f32)) as u32 & 255,
                _ => {
                    // 退场比例按更新次数收缩，零步更新也计数。
                    particle.scale *= 0.997;
                    integer(fade(255., 0., self.lives[2] as f32, phase_time as f32)) as u32 & 255
                }
            };
            for color in &mut particle.colors {
                let channels = std::array::from_fn::<_, 3, _>(|_| random.next_integer() % 200 + 56);
                *color = channels[0] | (channels[1] << 8) | (channels[2] << 16) | (alpha << 24);
            }
            if phase_time > self.lives[particle.phase as usize] {
                particle.phase += 1;
                particle.phase_time = particle.age;
            }
            particle.age = particle.age.wrapping_add(integer(elapsed));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (quad, particle) in host.quads.iter_mut().zip(&self.particles) {
            let region = [[0., 0.], [0., 0.5], [0.5, 0.]][particle.region as usize];
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
                vertex.color = particle.colors[i];
                vertex.uv = [
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

    #[test]
    fn energy_keeps_curve_stages_random_colors_zero_step_shrink_and_terminal_state()
    -> crate::Result<()> {
        let mut host = ParticleHost::default();
        host.reset_quads(1, [64., 128.])?;
        for (i, value) in [-40f32, 390., 640., 780., 90., 30., 600., 120., 1., 20., 30.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        let mut effect = Energy::new(&host, [64., 128.], &mut random);
        let checkpoints = [
            (
                0,
                [1, 0, 0],
                1,
                0x3f73d8d0,
                [0xc2200000, 0x43c30000, 0],
                [0xffffff; 4],
            ),
            (
                19,
                [30, 29, 2],
                229,
                0x3f73d8d0,
                [0xc340e622, 0x44170af8, 0],
                [0xff7ac93b, 0xff79b18e, 0xff557eb4, 0xff49595c],
            ),
            (
                20,
                [31, 29, 2],
                241,
                0x3f731d89,
                [0xc31827b0, 0x44197a80, 0],
                [0xf693d781, 0xf6c0f2a0, 0xf65b74c8, 0xf66074ad],
            ),
            (
                21,
                [31, 29, 2],
                253,
                0x3f7262d2,
                [0xc2dde7a8, 0x441bdbe8, 0],
                [0xeeb97765, 0xee49fa64, 0xee8167aa, 0xeea5bbae],
            ),
            (
                41,
                [61, 60, 3],
                493,
                0x3f643ff7,
                [0x44200000, 0x44430000, 0],
                [0xb98ede, 0x48788d, 0xddb248, 0x7d6b73],
            ),
            (
                60,
                [61, 60, 3],
                493,
                0x3f643ff7,
                [0x44200000, 0x44430000, 0],
                [0xb98ede, 0x48788d, 0xddb248, 0x7d6b73],
            ),
        ];
        let mut consumed = 0;
        for _ in 0..5 {
            expected.next_float();
        }
        for step in 0..=60 {
            if step != 0 {
                let delta = if step % 9 == 0 {
                    7
                } else if step % 7 == 0 {
                    0
                } else {
                    1
                };
                effect.update(delta, &mut random);
            }
            if let Some(&(_, state, calls, scale, position, colors)) =
                checkpoints.iter().find(|c| c.0 == step)
            {
                let particle = &effect.particles[0];
                assert_eq!(
                    [particle.age, particle.phase_time, particle.phase as i32],
                    state
                );
                assert_eq!(particle.position.map(f32::to_bits), position);
                assert_eq!(particle.scale.to_bits(), scale);
                assert_eq!(particle.colors, colors);
                for _ in consumed..calls {
                    expected.next_integer();
                }
                consumed = calls;
                assert_eq!(
                    random.clone().next_integer(),
                    expected.clone().next_integer()
                );
                assert_eq!(random.clone().next_float(), expected.clone().next_float());
            }
        }
        effect.write_mesh(&mut host);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.], [0.5, 0.5]]
        );
        assert_eq!(host.quads[0].map(|v| v.color), checkpoints[5].5);
        let degenerate = Path::new([[0.; 3]; 3], 6.);
        assert!(
            degenerate
                .tangents
                .into_iter()
                .flatten()
                .all(|n| n.to_bits() == 0xffc0_0000)
        );
        assert_eq!(degenerate.at(1.), [0.; 3]);
        assert_eq!(Path::new([[0.; 3]; 3], 0.).tangents, [[0.; 3]; 3]);
        host.parameters[8] = 2147483520f32.to_bits();
        host.parameters[9] = 128f32.to_bits();
        host.parameters[10] = 128f32.to_bits();
        let overflowing = Energy::new(&host, [64., 128.], &mut random);
        assert_eq!(overflowing.particles[0].path.duration, -2147483520f32);
        host.reset_quads(0, [64., 128.])?;
        let mut expected = random.clone();
        let mut empty = Energy::new(&host, [64., 128.], &mut random);
        empty.update(0, &mut random);
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        Ok(())
    }
}
