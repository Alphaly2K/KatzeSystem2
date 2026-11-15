use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(n: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&n) {
        n as i32
    } else {
        i32::MIN
    }
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    // 归一化先累加平方再写回单精度，保持路径控制点的舍入边界。
    let q = v.map(|n| n as f64 * n as f64);
    let len = ((((q[0] + q[1]) + q[2]) as f32) as f64).sqrt() as f32;
    if len > 0. { v.map(|n| n / len) } else { v }
}

fn linear(from: f32, to: f32, end: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= end {
        to
    } else {
        (time / end) * (to - from) + from
    }
}

struct Sprite {
    position: [f32; 3],
    origin: [f32; 3],
    angle: f32,
    scale: [f32; 3],
    color: u32,
    region: u32,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    sprite: Sprite,
}

pub(super) struct SceneSparkle {
    lives: [i32; 3],
    image: [f32; 2],
    heads: Vec<Head>,
}

impl SceneSparkle {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let lives = [p[15], p[16], p[17]].map(|n| integer(n as f64));
        let total = lives.into_iter().fold(0i32, i32::wrapping_add);
        let curves: [Path; 2] = std::array::from_fn(|i| {
            Path::new_extended(
                (0..3)
                    .map(|j| [p[1 + i * 6 + j * 2], p[2 + i * 6 + j * 2], 0.])
                    .collect(),
                100.,
                if i == 0 { p[18] } else { 100. },
            )
        });
        let scale = (p[13] as f64 / 100.) as f32;
        let angle = (20f64 * std::f32::consts::PI as f64 / 180.) as f32;
        let sin = (angle as f64).sin() as f32;
        let cos = (angle as f64).cos() as f32;
        let stagger = if integer(p[19] as f64) != 0 {
            host.quads.len() / 2
        } else {
            0
        };
        let heads = (0..host.quads.len())
            .map(|i| {
                let region = random.next_integer() & 3;
                let size = (random.next_float() as f64 * scale as f64 + scale as f64) as f32;
                let time = (random.next_float() as f64 * 100.) as f32;
                let a = curves[0].at(time);
                let b = curves[1].at(time);
                let x = linear(a[0], b[0], 100., (random.next_float() as f64 * 100.) as f32);
                let y = linear(a[1], b[1], 100., (random.next_float() as f64 * 100.) as f32);
                let z = (random.next_float() as f64 * 50.) as f32;
                let rotation = (random.next_float() as f64 * std::f32::consts::PI as f64) as f32;
                let start = [x, y, z];
                let dir = normalized([
                    (random.next_float() as f64 * 30. - 15.) as f32,
                    (random.next_float() as f64 * 30. - 15.) as f32,
                    0.,
                ]);
                let distance = (random.next_float() as f64 * p[14] as f64 + p[14] as f64) as f32;
                let scaled = dir.map(|n| n * distance);
                let mut point = std::array::from_fn(|j| start[j] + scaled[j]);
                let direction = normalized(std::array::from_fn(|j| point[j] - start[j]));
                let mut points = vec![start, point];
                let low = [
                    (cos as f64 * direction[0] as f64 + sin as f64 * direction[1] as f64) as f32,
                    (-sin as f64 * direction[0] as f64 + cos as f64 * direction[1] as f64) as f32,
                ];
                let high = [
                    (cos as f64 * direction[0] as f64 - sin as f64 * direction[1] as f64) as f32,
                    (sin as f64 * direction[0] as f64 + cos as f64 * direction[1] as f64) as f32,
                ];
                let spread = std::array::from_fn::<_, 2, _>(|j| {
                    let d = high[j] - low[j];
                    ((d as f64 * d as f64) as f32 as f64).sqrt() as f32
                });
                for _ in 0..4 {
                    let perturb = spread
                        .map(|s| (random.next_float() as f64 * s as f64 - s as f64 * 0.5) as f32);
                    let dir =
                        normalized([direction[0] + perturb[0], direction[1] + perturb[1], 0.]);
                    let distance =
                        (random.next_float() as f64 * p[14] as f64 + p[14] as f64) as f32;
                    let scaled = dir.map(|n| n * distance);
                    point = std::array::from_fn(|j| point[j] + scaled[j]);
                    points.push(point);
                    if random.next_integer().is_multiple_of(15) {
                        break;
                    }
                }
                let path = Path::new_extended(points, total as f32, p[18]);
                let age = if i < stagger {
                    integer(random.next_float() as f64 * total as f64)
                } else {
                    0
                };
                Head {
                    age,
                    phase_time: 0,
                    phase: 0,
                    path,
                    sprite: Sprite {
                        position: start,
                        origin: [image[0] * 0.5, image[1] * 0.5, 0.],
                        angle: rotation,
                        scale: [size, size, 1.],
                        color: 0xffffff,
                        region,
                    },
                }
            })
            .collect();
        Self {
            lives,
            image,
            heads,
        }
    }

    pub(super) fn update(&mut self, delta: u32) {
        let step = integer(delta as f32 as f64);
        for h in &mut self.heads {
            let elapsed = h.age.wrapping_sub(h.phase_time);
            if h.phase < 3 {
                h.sprite.position = h.path.at(h.age as f32);
            }
            let alpha = match h.phase {
                0 => Some(integer(
                    linear(0., 128., self.lives[0] as f32, h.age as f32) as f64,
                )),
                2 => Some(integer(
                    linear(128., 0., self.lives[2] as f32, elapsed as f32) as f64,
                )),
                _ => None,
            };
            if let Some(alpha) = alpha {
                h.sprite.color = (h.sprite.color & 0xffffff) | ((alpha as u32 & 255) << 24);
            }
            if h.phase < 3
                && (if h.phase == 0 { h.age } else { elapsed }) > self.lives[h.phase as usize]
            {
                h.phase += 1;
                h.phase_time = h.age;
            }
            h.age = h.age.wrapping_add(step);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        for (i, p) in self.heads.iter().enumerate() {
            let s = &p.sprite;
            let mut transform = Transform::default();
            transform.set_scale(s.scale);
            transform.set_rotation([0., 0., s.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&s.position);
            for (j, v) in host.quads[i].iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        (j & 1) as f32 * self.image[0] - s.origin[0],
                        (j >> 1) as f32 * self.image[1] - s.origin[1],
                        -s.origin[2],
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = s.color;
                v.uv = [
                    ((s.region & 1) + (j & 1) as u32) as f32 * 0.5,
                    ((s.region >> 1) + (j >> 1) as u32) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(count: usize) -> ParticleHost {
        let mut host = ParticleHost::default();
        host.reset_quads(count, [64., 128.]).unwrap();
        for (i, value) in [
            9f32, 12.5, -8., -35., 220., 137., 7.8, 782., 169., 44., -120., -60., -3., 137., 15.25,
            7.8, 0., 3.5, 45., 1.,
        ]
        .into_iter()
        .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        host
    }

    fn assert_random(random: &ScriptRandom, integers: usize, floats: usize) {
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..integers {
            expected.next_integer();
        }
        for _ in 0..floats {
            expected.next_float();
        }
        let mut actual = random.clone();
        assert_eq!(actual.next_integer(), expected.next_integer());
        assert_eq!(
            actual.next_float().to_bits(),
            expected.next_float().to_bits()
        );
    }

    #[test]
    fn scene_sparkle_preserves_curve_rounding_strict_phases_and_terminal_geometry() {
        let mut host = host(1);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = SceneSparkle::new(&host, [64., 128.], &mut random);
        let h = &effect.heads[0];
        assert_eq!(h.path.points.len(), 6);
        assert_eq!(
            h.path.points[0].map(f32::to_bits),
            [0x43ca5cc4, 0x42603d18, 0x3f38b5bd]
        );
        assert_eq!(
            h.path.points[5].map(f32::to_bits),
            [0x43ea51c6, 0x42ef8165, 0x3f38b5bd]
        );
        assert_eq!(
            h.path.tangents[1].map(f32::to_bits),
            [0x41fab2d0, 0x42012e2d, 0]
        );
        assert_eq!(h.path.total.to_bits(), 1119148191);
        assert_random(&random, 5, 21);
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert!(host.quads[0].iter().all(|v| v.color >> 24 == 0));
        for (delta, age, phase, alpha) in [
            (7, 7, 0, 0),
            (1, 8, 0, 128),
            (0, 8, 1, 128),
            (0, 8, 1, 128),
            (1, 9, 1, 128),
            (0, 9, 2, 128),
            (3, 12, 2, 128),
            (0, 12, 2, 0),
            (1, 13, 2, 0),
            (0, 13, 3, 0),
        ] {
            effect.update(delta);
            let h = &effect.heads[0];
            assert_eq!((h.age, h.phase, h.sprite.color >> 24), (age, phase, alpha));
        }
        effect.write_mesh(&mut host);
        let last = host.quads.clone();
        effect.update(100);
        effect.update(0);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads, last);
        assert_random(&random, 5, 21);
    }

    #[test]
    fn scene_sparkle_staggers_half_the_heads_and_handles_empty_and_zero_duration_paths() {
        let host = host(7);
        let mut random = ScriptRandom::seeded(17);
        let effect = SceneSparkle::new(&host, [64., 128.], &mut random);
        assert_eq!(
            effect.heads.iter().map(|h| h.age).collect::<Vec<_>>(),
            [1, 0, 0, 0, 0, 0, 0]
        );
        assert_random(&random, 35, 150);
        let mut empty = ParticleHost::default();
        empty.reset_quads(0, [64., 128.]).unwrap();
        let mut effect = SceneSparkle::new(&empty, [64., 128.], &mut random);
        effect.update(u32::MAX);
        effect.write_mesh(&mut empty);
        assert_random(&random, 35, 150);
        empty.reset_quads(1, [64., 128.]).unwrap();
        let mut effect = SceneSparkle::new(&empty, [64., 128.], &mut random);
        let h = &effect.heads[0];
        assert_eq!(h.path.duration, 0.);
        assert!(h.path.points.windows(2).all(|p| p[0] == p[1]));
        assert!(h.path.tangents.iter().all(|&t| t == [0.; 3]));
        effect.update(1);
        effect.update(0);
        effect.update(1);
        effect.update(0);
        effect.update(1);
        effect.update(0);
        assert_eq!(effect.heads[0].phase, 3);
        assert_eq!(effect.heads[0].sprite.color >> 24, 0);
    }
}
