use super::Transform;
use crate::kcs::{ParticleHost, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn length(delta: [f32; 3]) -> f32 {
    let squares = delta.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
}

struct Path {
    points: [[f32; 3]; 3],
    tangents: [[f32; 3]; 3],
    lengths: [f32; 2],
    total: f32,
    duration: f32,
}

impl Path {
    fn new(anchor: [f32; 3], angle: f32, distance: f32, duration: f32) -> Self {
        // 方向角先量化到千分之一弧度，再生成两段曲线的控制点。
        let direction = |angle: f32| {
            let quantized = integer(angle * 1000.) as f32 / 1000.;
            [
                (quantized as f64).sin() as f32,
                -(quantized as f64).cos() as f32,
                0.,
            ]
        };
        let first = direction(angle);
        let second = direction(angle * 0.85);
        let middle = std::array::from_fn(|i| first[i] * distance + anchor[i]);
        let end = std::array::from_fn(|i| (second[i] * distance) / 0.99 + middle[i]);
        let points = [anchor, middle, end];
        let lengths = std::array::from_fn(|i| {
            length(std::array::from_fn(|j| points[i + 1][j] - points[i][j]))
        });
        let total = lengths[0] + lengths[1];
        let tangents = if duration == 0. {
            [[0.; 3]; 3]
        } else {
            std::array::from_fn(|i| {
                let delta = match i {
                    0 => std::array::from_fn(|j| points[1][j] - points[0][j]),
                    1 => std::array::from_fn(|j| {
                        (points[2][j] - points[1][j]) - (points[0][j] - points[1][j])
                    }),
                    _ => std::array::from_fn(|j| points[2][j] - points[1][j]),
                };
                let size = length(delta);
                delta.map(|n| (n / size) * 25.)
            })
        };
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
        // 按总长度选择区间，保留边界判断前的逐步舍入。
        let distance = (time / self.duration) * self.total;
        let (index, previous) = if self.lengths[0] > distance {
            (0, 0.)
        } else if self.total > distance {
            (1, self.lengths[0])
        } else {
            return self.points[2];
        };
        let t = (distance - previous) / self.lengths[index];
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
                let values = [
                    self.points[index][j],
                    self.points[index + 1][j],
                    self.tangents[index][j],
                    self.tangents[index + 1][j],
                ];
                ((values[0] * row[0] + values[1] * row[1]) + values[2] * row[2])
                    + values[3] * row[3]
            })
        });
        std::array::from_fn(|j| ((c[0][j] * cube + c[1][j] * square) + c[2][j] * t) + c[3][j])
    }
}

#[derive(Clone)]
struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    origin: [f32; 3],
    position: [f32; 3],
    angle: f32,
    scale: f32,
    alpha: u32,
}

pub(super) struct Sigh {
    anchor: [f32; 3],
    mirror: bool,
    lives: [i32; 2],
    center: [f32; 2],
    image: [f32; 2],
    path: Path,
    particles: Vec<Particle>,
}

impl Sigh {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let params = host.parameters.map(f32::from_bits);
        let anchor = [params[0], params[1], 0.];
        let mirror = integer(params[2]) != 0;
        let mut scale = params[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let lives = [integer(params[4]), integer(params[5])];
        let initial_angle = if mirror { 2.2 } else { -2.2 };
        let path = Path::new(
            anchor,
            initial_angle,
            scale * 100.,
            lives[0].wrapping_add(lives[1]) as f32,
        );
        let particle = Particle {
            age: 0,
            phase_time: 0,
            phase: 0,
            origin: [0.; 3],
            position: anchor,
            angle: 0.,
            scale,
            alpha: 0,
        };
        Self {
            anchor,
            mirror,
            lives,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            path,
            particles: vec![particle; host.quads.len()],
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let Some((head, trails)) = self.particles.split_first_mut() else {
            return;
        };
        let point = self.path.at(head.age as f32);
        head.position[0] = point[0];
        head.position[1] = point[1];
        if head.phase < 2 {
            let elapsed = head.age.wrapping_sub(head.phase_time);
            if head.phase == 0 {
                head.alpha =
                    integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32 & 255;
            }
            let mut delta = std::array::from_fn(|i| point[i] - self.anchor[i]);
            let size = length(delta);
            if size > 0. {
                delta = delta.map(|n| n / size);
            }
            let dot = (delta[0] * 0. + delta[1]) + delta[2] * 0.;
            let mut angle = (dot as f64).acos() as f32;
            if (delta[0] + delta[1] * 0.) + delta[2] * 0. > 0. {
                angle *= -1.;
            }
            head.angle = if self.mirror {
                angle + 0.8
            } else {
                angle - 0.8
            };
            if elapsed > self.lives[head.phase as usize] {
                head.phase = if head.phase == 0 { 1 } else { 10 };
                head.phase_time = head.age;
            }
        }
        if head.phase != 10 {
            if let Some(trail) = trails.iter_mut().find(|p| p.phase == 0) {
                trail.origin = head.position;
                trail.position = head.position;
                trail.position[2] -= 1.;
                trail.angle = head.angle;
                trail.scale = head.scale;
                trail.phase = 1;
            }
        }
        let step = integer(delta as f32);
        for trail in trails {
            if trail.phase == 0 {
                continue;
            }
            trail.position[0] = trail.origin[0];
            trail.position[1] = trail.origin[1];
            trail.alpha = integer(fade(30., 0., 20., trail.age as f32)) as u32 & 255;
            trail.scale *= 1.005;
            trail.age = trail.age.wrapping_add(step);
            if trail.age > 20 {
                trail.alpha = 0;
                trail.age = 0;
                trail.phase = 0;
            }
            for (i, value) in offset.into_iter().enumerate() {
                trail.position[i] += value;
            }
        }
        for (i, value) in offset.into_iter().enumerate() {
            head.position[i] += value;
        }
        head.age = head.age.wrapping_add(step);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [width, height] = self.image;
        for (index, (particle, quad)) in self.particles.iter().zip(&mut host.quads).enumerate() {
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
                let u = if self.mirror { 1 - (i & 1) } else { i & 1 };
                vertex.uv = [
                    (usize::from(index != 0) + u) as f32 * 0.5,
                    (i >> 1) as f32 * 0.5,
                ];
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigh_keeps_curve_rounding_phase_boundaries_and_trail_decay() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(4, [64., 128.]).unwrap();
        for (i, value) in [1195f32, 369., 0., 29., 8., 3.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Sigh::new(&host, [64., 128.]);
        for step in 1u32..=40 {
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
            effect.update(delta, source);
            let expected = match step {
                2 => [2, 0, 0, 0x4493_aa95, 0x43bd_beff, 0x3df3_32d0, 31],
                10 => [15, 14, 1, 0x4490_f9fd, 0x43c5_59b7, 0x3e9d_675e, 255],
                25 => [34, 18, 10, 0x448e_21fd, 0x43c1_99b7, 0x3e9d_675e, 255],
                _ => continue,
            };
            let head = &effect.particles[0];
            assert_eq!(
                [
                    head.age as u32,
                    head.phase_time as u32,
                    head.phase,
                    head.position[0].to_bits(),
                    head.position[1].to_bits(),
                    head.angle.to_bits(),
                    head.alpha,
                ],
                expected
            );
            let trail = &effect.particles[1];
            let expected = match step {
                2 => [2, 1, 28, 0x3e95_f7f0],
                10 => [15, 1, 9, 0x3e9c_12c2],
                _ => [0, 0, 0, 0x3ea1_9e54],
            };
            assert_eq!(
                [
                    trail.age as u32,
                    trail.phase,
                    trail.alpha,
                    trail.scale.to_bits()
                ],
                expected
            );
        }
        assert!(
            effect.particles[1..]
                .iter()
                .all(|p| p.phase == 0 && p.alpha == 0)
        );
        let path = Path::new([10., 20., 0.], 2.2, 1., 0.);
        assert_eq!(path.at(0.), path.points[0]);
        assert_eq!(path.at(1.), path.points[2]);
    }
}
