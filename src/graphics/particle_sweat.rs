use crate::kcs::{ParticleHost, transform_vector};

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

struct Path {
    points: [[f32; 3]; 2],
    tangent: [f32; 3],
    length: f32,
    duration: f32,
}

impl Path {
    fn new(anchor: [f32; 3], distance: f32, duration: f32) -> Self {
        let points = [anchor, [anchor[0], anchor[1] + distance, 0.]];
        let delta: [f32; 3] = std::array::from_fn(|i| points[1][i] - points[0][i]);
        let squares = delta.map(|n| n * n);
        let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
        let tangent = if duration == 0. {
            [0.; 3]
        } else {
            delta.map(|n| (n / length) * 10.)
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
        // 先按总长度换算距离，再选择区间，保留边界前的浮点舍入。
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
        let c = basis.map(|row| {
            std::array::from_fn::<_, 3, _>(|j| {
                ((self.points[0][j] * row[0] + self.points[1][j] * row[1])
                    + self.tangent[j] * row[2])
                    + self.tangent[j] * row[3]
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
    alpha: u32,
}

pub(super) struct Sweat {
    center: [f32; 2],
    scale: f32,
    lives: [i32; 2],
    image: [f32; 2],
    path: Path,
    particles: Vec<Particle>,
}

impl Sweat {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1], 0.];
        let mut scale = p[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let lives = [integer(p[3]), integer(p[4])];
        let distance = (scale * 150.) * if image[1] > 256. { 1. } else { 0.5 };
        let path = Path::new(anchor, distance, lives[0].wrapping_add(lives[1]) as f32);
        Self {
            center: host.surface_size.map(|n| n as f32 * 0.5),
            scale,
            lives,
            image,
            path,
            particles: vec![
                Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    origin: [0.; 3],
                    position: anchor,
                    alpha: 0
                };
                host.quads.len()
            ],
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let Some((head, trails)) = self.particles.split_first_mut() else {
            return;
        };
        head.position[..2].copy_from_slice(&self.path.at(head.age as f32)[..2]);
        if head.phase < 2 {
            let elapsed = head.age.wrapping_sub(head.phase_time);
            if head.phase == 0 {
                head.alpha =
                    integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32 & 255;
            }
            if elapsed > self.lives[head.phase as usize] {
                head.phase = if head.phase == 0 { 1 } else { 10 };
                head.phase_time = head.age;
            }
        }
        if head.phase <= 1 {
            if let Some(trail) = trails.iter_mut().find(|p| p.phase == 0) {
                trail.origin = head.position;
                trail.position = head.position;
                trail.position[2] -= 1.;
                trail.phase = 1;
            }
        }
        let step = integer(delta as f32);
        for trail in trails {
            if trail.phase == 0 {
                continue;
            }
            trail.position[..2].copy_from_slice(&trail.origin[..2]);
            trail.alpha = integer(fade(30., 0., 22., trail.age as f32)) as u32 & 255;
            trail.age = trail.age.wrapping_add(step);
            if trail.age > 25 {
                trail.alpha = 0;
                trail.age = 0;
                trail.phase = 0;
            }
            for (i, n) in offset.into_iter().enumerate() {
                trail.position[i] += n;
            }
        }
        for (i, n) in offset.into_iter().enumerate() {
            head.position[i] += n;
        }
        head.age = head.age.wrapping_add(step);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut matrix = [0.; 16];
            matrix[0] = self.scale;
            matrix[5] = self.scale;
            matrix[10] = 1.;
            matrix[15] = 1.;
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
                v.color = 0x00ff_ffff | (p.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweat_keeps_curve_rounding_phase_boundaries_and_trail_recycling() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(4, [64., 128.]).unwrap();
        for (i, value) in [821f32, 166., 86., 20., 30.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Sweat::new(&host, [64., 128.]);
        for step in 1u32..=70 {
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
                7 => [6, 0, 0, 76, 0x444e_e000, 0x433f_ddb5],
                17 => [21, 0, 0, 255, 0x444e_1000, 0x433d_2f1b],
                40 => [59, 55, 10, 255, 0x444e_e000, 0x4357_8000],
                70 => [103, 55, 10, 255, 0x444c_7000, 0x437d_0000],
                _ => continue,
            };
            let head = &effect.particles[0];
            assert_eq!(
                [
                    head.age as u32,
                    head.phase_time as u32,
                    head.phase,
                    head.alpha,
                    head.position[0].to_bits(),
                    head.position[1].to_bits(),
                ],
                expected
            );
        }
        assert!(
            effect.particles[1..]
                .iter()
                .all(|p| p.phase == 0 && p.alpha == 0)
        );
        let short = Sweat::new(&host, [64., 256.]);
        let tall = Sweat::new(&host, [64., 257.]);
        assert_eq!(short.path.points[1][1], 230.5);
        assert_eq!(tall.path.points[1][1], 295.);
        let instant = Path::new([10., 20., 0.], 1., 0.);
        assert_eq!(instant.at(0.), instant.points[0]);
        assert_eq!(instant.at(1.), instant.points[1]);
    }
}
