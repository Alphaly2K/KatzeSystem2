use super::{
    Transform,
    particle_curve::{Path, TwoPointPath},
};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

// 段长先舍入各平方，方向归一化先累加平方再写回单精度。
fn segment(v: [f32; 3]) -> f32 {
    let q = v.map(|n| n * n);
    ((((q[0] as f64 + q[1] as f64) + q[2] as f64) as f32) as f64).sqrt() as f32
}

fn tangent_length(v: [f32; 3]) -> f32 {
    let q = v.map(|n| n as f64 * n as f64);
    ((((q[0] + q[1]) + q[2]) as f32) as f64).sqrt() as f32
}

fn integer(n: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&n) {
        n as i32
    } else {
        i32::MIN
    }
}

fn spawn_points(from: [f32; 3], to: [f32; 3], index: usize, span: f32) -> Vec<[f32; 3]> {
    let delta = std::array::from_fn::<_, 3, _>(|j| to[j] - from[j]);
    let rad = (span as f64 * std::f32::consts::PI as f64 / 180.) as f32;
    let low = rad * -0.5;
    let high = rad * 0.5;
    let time = (index as f64 * 16.666666f32 as f64) as f32;
    let angle = if time <= 0. {
        low
    } else if time >= 100. {
        high
    } else {
        ((time / 100.) * (high - low)) + low
    };
    let v = normalized(rotate(delta, angle));
    let mut distance = segment(delta);
    let mut points = vec![from];
    for _ in 0..4 {
        let scaled = v.map(|n| n * distance);
        points.push(std::array::from_fn(|j| from[j] + scaled[j]));
        distance = (distance as f64 * 1.01f32 as f64) as f32;
    }
    points
}

fn linear(from: f32, to: f32, start: f32, end: f32, time: f32) -> f32 {
    if time <= start {
        from
    } else if time >= end {
        to
    } else {
        ((time - start) / (end - start)) * (to - from) + from
    }
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let len = tangent_length(v);
    if len > 0. { v.map(|n| n / len) } else { v }
}

fn rotate(v: [f32; 3], angle: f32) -> [f32; 3] {
    let a = (integer(angle as f64 * 1000.) as f64 / 1000.) as f32;
    let c = (a as f64).cos() as f32;
    let s = (a as f64).sin() as f32;
    let m = [
        c, s, 0f32, 0., -s, c, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    std::array::from_fn(|i| {
        (((v[0] as f64 * m[i] as f64 + v[1] as f64 * m[i + 4] as f64)
            + v[2] as f64 * m[i + 8] as f64)
            + m[i + 12] as f64) as f32
    })
}

struct Sprite {
    position: [f32; 3],
    origin: [f32; 3],
    angle: f32,
    scale: [f32; 3],
    color: u32,
    region: Option<u32>,
}

impl Sprite {
    fn new() -> Self {
        Self {
            position: [0.; 3],
            origin: [0.; 3],
            angle: 0.,
            scale: [1.; 3],
            color: 0xffffff,
            region: None,
        }
    }
}

struct Trail {
    age: i32,
    active: bool,
    path: Option<TwoPointPath>,
    sprite: Sprite,
    angle: f32,
    opacity: u32,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    path: Path,
    previous: [f32; 3],
    scale: f32,
    sprite: Sprite,
    trails: Vec<Trail>,
}

pub(super) struct Ice {
    life: i32,
    image: [f32; 2],
    trail_life: i32,
    emission_limit: i32,
    scale_decay: f64,
    spin_step: f64,
    relative_wait: bool,
    heads: Vec<Head>,
}

impl Ice {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        Self::new_variant(host, image, random, false)
    }

    pub(super) fn new_star(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Self {
        Self::new_variant(host, image, random, true)
    }

    fn new_variant(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        star: bool,
    ) -> Self {
        let count = host.quads.len();
        let parameters = host.parameters.map(f32::from_bits);
        let group = count / 7;
        let groups = count.checked_div(group).unwrap_or(count);
        let anchor = [parameters[0], parameters[1], 0.];
        let target = [parameters[2], parameters[3], 0.];
        let scale = (parameters[4] as f64 / 100.) as f32;
        let life = integer(parameters[5] as f64);
        let palette = [
            0xf0f0f0, 0xffff0a, 0xff0aff, 0x0affff, 0xff0a0a, 0x0aff0a, 0x0a0aff,
        ];
        let heads = (0..groups)
            .map(|i| {
                let points =
                    spawn_points(anchor, target, i, if star { parameters[6] } else { 140. });
                let total = points
                    .windows(2)
                    .map(|p| segment(std::array::from_fn(|j| p[1][j] - p[0][j])))
                    .fold(0f32, |a, b| a + b);
                let path = Path::new_extended(points, life as f32, total);
                let mut sprite = Sprite::new();
                sprite.position = anchor;
                let trails = (0..group.saturating_sub(1))
                    .map(|_| {
                        let angle =
                            (random.next_float() as f64 * std::f32::consts::PI as f64) as f32;
                        let region = ((random.next_integer() & 3) + 2).min(3);
                        let mut sprite = Sprite::new();
                        sprite.origin = [image[0] * 0.5, image[1] * 0.5, 0.];
                        sprite.color = palette.get(i).copied().unwrap_or(0);
                        sprite.region = Some(region);
                        Trail {
                            age: 0,
                            active: false,
                            path: None,
                            sprite,
                            angle,
                            opacity: 0,
                        }
                    })
                    .collect();
                Head {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    path,
                    previous: anchor,
                    scale,
                    sprite,
                    trails,
                }
            })
            .collect();
        Self {
            life,
            image,
            trail_life: if star { 200 } else { 120 },
            emission_limit: if star {
                integer(parameters[7] as f64)
            } else {
                16
            },
            scale_decay: if star { 0.99f32 as f64 } else { 0.94f32 as f64 },
            spin_step: if star { 0.04f32 as f64 } else { 0.04 },
            relative_wait: star,
            heads,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let step = integer(delta as f32 as f64);
        for h in &mut self.heads {
            let waiting_age = if self.relative_wait {
                h.age.wrapping_sub(h.phase_time)
            } else {
                h.age
            };
            if h.phase == 0 {
                h.sprite.position = h.path.at(h.age as f32);
                if h.age > self.life {
                    h.phase = 1;
                    h.phase_time = h.age;
                }
            } else if h.phase == 1 && waiting_age as f64 > 60. {
                h.phase = 2;
                h.phase_time = h.age;
            }
            h.scale = (h.scale as f64 * self.scale_decay) as f32;
            if h.phase == 0 {
                let mut emitted = 0;
                for t in &mut h.trails {
                    if t.active {
                        continue;
                    }
                    let ix = linear(
                        h.previous[0],
                        h.sprite.position[0],
                        0.,
                        1.,
                        random.next_float(),
                    );
                    let iy = linear(
                        h.previous[1],
                        h.sprite.position[1],
                        0.,
                        1.,
                        random.next_float(),
                    );
                    let x = ((random.next_float() as f64 * 100. - 50.) * (h.scale as f64 * 2.)
                        + ix as f64) as f32;
                    let y = ((random.next_float() as f64 * 100. - 50.) * (h.scale as f64 * 2.)
                        + iy as f64) as f32;
                    let z = (random.next_float() as f64 * 60. * (h.scale as f64 * 2.)
                        + h.sprite.position[2] as f64) as f32;
                    let start = [x, y, z];
                    t.sprite.position = start;
                    t.sprite.angle = h.sprite.angle;
                    let mut dir = std::array::from_fn(|i| h.previous[i] - h.sprite.position[i]);
                    if tangent_length(dir) < 0.001 {
                        let a = h.path.at(t.age as f32);
                        let b = h.path.at(t.age.wrapping_add(1) as f32);
                        dir = std::array::from_fn(|i| b[i] - a[i]);
                    }
                    dir = normalized(dir);
                    let rad = (30f64 * std::f32::consts::PI as f64 / 180.) as f32;
                    let time = (random.next_float() as f64 * 100.) as f32;
                    let angle = linear(-rad, rad, 0., 100., time);
                    dir = normalized(rotate(dir, angle));
                    let distance = (random.next_float() as f64 * 50. + 50.) as f32;
                    let scaled = dir.map(|n| n * distance);
                    let end = std::array::from_fn(|i| h.sprite.position[i] + scaled[i]);
                    let len = segment(std::array::from_fn(|i| end[i] - start[i]));
                    t.path = Some(TwoPointPath::new_extended(
                        [start, end],
                        self.trail_life as f32,
                        (len as f64 / 10.) as f32,
                    ));
                    let s = if t.sprite.region == Some(3) {
                        (h.scale as f64 * 0.7f32 as f64) as f32
                    } else {
                        h.scale
                    };
                    t.sprite.scale = [s, s, h.sprite.scale[2]];
                    t.angle = (random.next_float() as f64 * std::f32::consts::PI as f64) as f32;
                    t.opacity = random.next_integer() % 200;
                    t.active = true;
                    emitted += 1;
                    if emitted > self.emission_limit {
                        break;
                    }
                }
            }
            for t in &mut h.trails {
                if !t.active {
                    continue;
                }
                t.sprite.position = t.path.as_ref().unwrap().at(t.age as f32);
                t.angle = (random.next_float() as f64 * self.spin_step + t.angle as f64) as f32;
                t.sprite.angle = t.angle;
                for n in &mut t.sprite.scale[..2] {
                    *n = (*n as f64 * 0.99f32 as f64) as f32;
                }
                let opacity = integer(linear(
                    t.opacity as f32,
                    0.,
                    0.,
                    self.trail_life as f32,
                    t.age as f32,
                ) as f64);
                let alpha = integer(
                    (random.next_float() as f64 * 0.6f32 as f64) * opacity as f64
                        + opacity as f64 * 0.4f32 as f64,
                ) as u32
                    & 255;
                t.sprite.color = (t.sprite.color & 0xffffff) | (alpha << 24);
                t.age = t.age.wrapping_add(step);
                if t.age >= self.trail_life {
                    t.age = 0;
                    t.active = false;
                    t.sprite.color &= 0xffffff;
                }
            }
            h.previous = h.sprite.position;
            h.age = h.age.wrapping_add(step);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        for (i, p) in self.heads.iter().enumerate() {
            let start = self.heads.len() + i * p.trails.len();
            let sprites = std::iter::once((i, &p.sprite)).chain(
                p.trails
                    .iter()
                    .enumerate()
                    .map(|(j, t)| (start + j, &t.sprite)),
            );
            for (index, s) in sprites {
                let mut transform = Transform::default();
                transform.set_scale(s.scale);
                transform.set_rotation([0., 0., s.angle]);
                let mut matrix = *transform.matrix();
                matrix[12..15].copy_from_slice(&s.position);
                for (j, v) in host.quads[index].iter_mut().enumerate() {
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
                    v.uv = if let Some(region) = s.region {
                        [
                            ((region & 1) + (j & 1) as u32) as f32 * 0.5,
                            ((region >> 1) + (j >> 1) as u32) as f32 * 0.5,
                        ]
                    } else {
                        [(j & 1) as f32, (j >> 1) as f32]
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(count: usize, scale: f32, life: f32) -> ParticleHost {
        let mut host = ParticleHost::default();
        host.reset_quads(count, [64., 128.]).unwrap();
        for (i, n) in [210f32, 560., 900., -800., scale, life]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        host
    }

    fn assert_random(actual: &ScriptRandom, integers: usize, floats: usize) {
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..integers {
            expected.next_integer();
        }
        for _ in 0..floats {
            expected.next_float();
        }
        let mut actual = actual.clone();
        assert_eq!(actual.next_integer(), expected.next_integer());
        assert_eq!(
            actual.next_float().to_bits(),
            expected.next_float().to_bits()
        );
    }

    #[test]
    fn star_uses_configured_fan_and_emission_limit_with_relative_wait_and_longer_trails() {
        let mut host = host(140, 15., 10.);
        for (i, n) in [0f32, 620., 640., 100., 15., 10., 100., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Ice::new_star(&host, [64., 128.], &mut random);
        assert_eq!(effect.heads.len(), 7);
        assert!(effect.heads.iter().all(|h| h.trails.len() == 19));
        assert_eq!(
            effect.heads[0].path.points[4].map(f32::to_bits),
            [1096811233, 3278208536, 0]
        );
        effect.update(0, &mut random);
        assert!(
            effect
                .heads
                .iter()
                .all(|h| { h.age == 0 && h.trails.iter().filter(|t| t.active).count() == 6 })
        );
        assert_eq!(
            effect.heads[0].scale,
            (0.15f32 as f64 * 0.99f32 as f64) as f32
        );
        for delta in [10, 1, 0] {
            effect.update(delta, &mut random);
        }
        assert!(
            effect
                .heads
                .iter()
                .all(|h| h.phase == 1 && h.phase_time == 11)
        );
        for delta in [60, 1] {
            effect.update(delta, &mut random);
            assert!(effect.heads.iter().all(|h| h.phase == 1));
        }
        effect.update(0, &mut random);
        assert!(
            effect
                .heads
                .iter()
                .all(|h| h.phase == 2 && h.phase_time == 72)
        );
        effect.update(137, &mut random);
        assert!(
            effect
                .heads
                .iter()
                .all(|h| h.trails.iter().any(|t| t.active))
        );
        effect.update(1, &mut random);
        assert!(effect.heads.iter().all(|h| {
            h.trails
                .iter()
                .all(|t| !t.active && t.age == 0 && t.sprite.color >> 24 == 0)
        }));
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert!(host.quads.iter().flatten().all(|v| v.color >> 24 == 0));
    }

    #[test]
    fn ice_limits_emission_keeps_zero_step_motion_and_drains_without_respawning() {
        let mut host = host(560, 15., 10.);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Ice::new(&host, [64., 128.], &mut random);
        assert_eq!(effect.heads.len(), 7);
        assert!(effect.heads.iter().all(|h| h.trails.len() == 79));
        assert_random(&random, 553, 553);
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert!(host.quads.iter().flatten().all(|v| v.color >> 24 == 0));
        effect.update(1, &mut random);
        assert_eq!(
            effect
                .heads
                .iter()
                .flat_map(|h| &h.trails)
                .filter(|t| t.active)
                .count(),
            119
        );
        let t = &effect.heads[0].trails[0];
        assert_eq!(
            t.sprite.position.map(f32::to_bits),
            [0x4349_040c, 0x440a_cced, 0x40a9_e8e6]
        );
        assert_eq!(t.sprite.angle.to_bits(), 0x3fe3_7f98);
        assert_eq!(t.sprite.scale[0].to_bits(), 0x3e0e_f0af);
        assert_eq!(t.sprite.color >> 24, 95);
        assert_random(&random, 672, 1743);
        let mut held_position = [0.; 3];
        let mut held_scale = 0.;
        let mut held_angle = 0.;
        for step in 2u32..=100 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, &mut random);
            let t = &effect.heads[0].trails[0];
            if step == 7 {
                assert_eq!(t.age, 6);
                assert_eq!(
                    t.sprite.position.map(f32::to_bits),
                    [0x4348_9154, 0x440a_b098, 0x40a7_f399]
                );
                held_position = t.sprite.position;
                held_scale = t.sprite.scale[0];
                held_angle = t.angle;
            } else if step == 8 {
                assert_eq!(t.sprite.position, held_position);
                assert!(t.sprite.scale[0] < held_scale);
                assert!(t.angle > held_angle);
            }
        }
        assert!(
            effect
                .heads
                .iter()
                .all(|h| h.phase == 2 && h.phase_time == 61)
        );
        assert!(
            effect
                .heads
                .iter()
                .flat_map(|h| &h.trails)
                .all(|t| !t.active && t.age == 0 && t.sprite.color >> 24 == 0)
        );
        effect.update(100, &mut random);
        assert_random(&random, 1106, 92519);
    }

    #[test]
    fn odd_groups_keep_remainders_and_negative_lifetimes_stop_emission_before_birth() {
        let mut random = ScriptRandom::seeded(17);
        let mut partial = host(29, 15., 10.);
        let last = partial.quads[28];
        let mut effect = Ice::new(&partial, [64., 128.], &mut random);
        effect.update(1, &mut random);
        effect.write_mesh(&mut partial);
        assert_eq!(partial.quads[28], last);
        let effect = Ice::new(&host(20, 15., 10.), [64., 128.], &mut random);
        assert_eq!(effect.heads.len(), 10);
        assert!(
            effect.heads[7..]
                .iter()
                .flat_map(|h| &h.trails)
                .all(|t| t.sprite.color == 0)
        );
        let mut effect = Ice::new(&host(21, -60., -3.), [64., 128.], &mut random);
        let before = random.clone();
        effect.update(60, &mut random);
        effect.update(0, &mut random);
        assert!(effect.heads.iter().all(|h| h.phase == 1 && h.age == 60));
        effect.update(1, &mut random);
        effect.update(0, &mut random);
        assert!(
            effect
                .heads
                .iter()
                .all(|h| h.phase == 2 && h.phase_time == 61)
        );
        assert!(
            effect
                .heads
                .iter()
                .flat_map(|h| &h.trails)
                .all(|t| !t.active)
        );
        let mut empty = host(0, 0., 0.);
        let mut effect = Ice::new(&empty, [64., 128.], &mut random);
        effect.update(0, &mut random);
        effect.write_mesh(&mut empty);
        let mut before = before;
        assert_eq!(random.next_integer(), before.next_integer());
        assert_eq!(random.next_float(), before.next_float());
    }
}
