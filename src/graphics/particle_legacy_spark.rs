use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[derive(Clone, Copy)]
enum Phase {
    Travel,
    Fade,
    Done,
}

struct Head {
    age: i32,
    phase_start: i32,
    phase: Phase,
    path: Path,
    position: [f32; 3],
    angle: f32,
    scale: f32,
    alpha: u8,
    region: u32,
}

pub(super) struct LegacySpark {
    image: [f32; 2],
    heads: Vec<Head>,
    module_state: Arc<AtomicU32>,
}

fn truncate_i32(value: f64) -> i32 {
    if (-2147483648. ..2147483648.).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn normalized([x, y]: [f32; 2]) -> [f32; 2] {
    let sum = (f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32;
    let length = f64::from(sum).sqrt() as f32;
    if length > 0. {
        [x / length, y / length]
    } else {
        [x, y]
    }
}

fn spawn(
    random: &mut ScriptRandom,
    duration: f32,
    center: [f32; 2],
    step: f32,
    scale: f32,
) -> Head {
    let region = random.next_integer() % 3;
    let scale = (f64::from(random.next_float()) * f64::from(scale) + f64::from(scale)) as f32;
    let start = [
        (f64::from(random.next_float()) * 200. - 100. + f64::from(center[0])) as f32,
        center[1],
        0.,
    ];
    let x = (f64::from(random.next_float()) * 0.3f32 as f64 - 0.15f32 as f64) as f32;
    let y = -(random.next_float() + 0.05);
    let direction = normalized([x, y]);
    let advance = |point: [f32; 3], direction: [f32; 2], random: &mut ScriptRandom| {
        let distance = (f64::from(random.next_float()) * f64::from(step) + f64::from(step)) as f32;
        [
            point[0] + direction[0] * distance,
            point[1] + direction[1] * distance,
            0.,
        ]
    };
    let mut point = advance(start, direction, random);
    let [x, y] = normalized([point[0] - start[0], point[1] - start[1]]);
    let angle = (50. * std::f32::consts::PI as f64 / 180.) as f32;
    let (sin, cos) = f64::from(angle).sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let lo = [
        ((cos * x) as f64 + sin as f64 * y as f64) as f32,
        ((cos * y) as f64 - sin as f64 * x as f64) as f32,
    ];
    let hi = [
        ((cos * x) as f64 - sin as f64 * y as f64) as f32,
        ((cos * y) as f64 + sin as f64 * x as f64) as f32,
    ];
    let spread = std::array::from_fn::<_, 2, _>(|i| (hi[i] - lo[i]).abs());
    let mut points = vec![start, point];
    for _ in 0..4 {
        let [dx, dy] = spread
            .map(|s| (f64::from(random.next_float()) * f64::from(s) - f64::from(s * 0.5)) as f32);
        point = advance(point, normalized([x + dx, y + dy]), random);
        points.push(point);
        if random.next_integer().is_multiple_of(15) {
            break;
        }
    }
    Head {
        age: 0,
        phase_start: 0,
        phase: Phase::Travel,
        path: Path::new_extended(points, duration, 400.),
        position: start,
        angle: 0.,
        scale,
        alpha: 0,
        region,
    }
}

impl LegacySpark {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Self {
        let [duration, x, y, step, percent, ..] = host.parameters.map(f32::from_bits);
        let heads = (0..host.quads.len())
            .map(|_| spawn(random, duration, [x, y], step, percent / 100.))
            .collect();
        Self {
            image,
            heads,
            module_state,
        }
    }

    pub(super) fn update(&mut self, delta: u32, duration: f32, random: &mut ScriptRandom) {
        let increment = u32::from(random.next_integer().is_multiple_of(2));
        let _ = self
            .module_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some((count + increment).min(self.heads.len() as u32))
            });
        let delta = truncate_i32(f64::from(delta as f32));
        let fade_after = truncate_i32((f64::from(duration * 0.7) + 0.5).floor());
        // The original updates every head; the module counter does not gate the loop.
        for head in &mut self.heads {
            let age = head.age as f32;
            match head.phase {
                Phase::Travel => {
                    head.position = head.path.at(age);
                    head.angle += random.next_float();
                    head.alpha = 255;
                    if head.age > fade_after {
                        head.phase = Phase::Fade;
                        head.phase_start = head.age;
                    }
                }
                Phase::Fade => {
                    head.position = head.path.at(age);
                    let start = head.phase_start as f32;
                    head.alpha = if age <= start {
                        255
                    } else if age >= duration {
                        0
                    } else {
                        ((age - start) / (duration - start) * -255. + 255.) as u8
                    };
                    if f64::from(head.age) > f64::from(duration) {
                        head.phase = Phase::Done;
                        head.phase_start = head.age;
                    }
                }
                Phase::Done => {}
            }
            head.age = head.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [w, h] = self.image;
        for (head, quad) in self.heads.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([head.scale, head.scale, 1.]);
            transform.set_rotation([0., 0., head.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&head.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let x = (i & 1) as u32;
                let y = (i >> 1) as u32;
                let point =
                    transform_vector([(x as f32 - 0.5) * w, (y as f32 - 0.5) * h, 0., 1.], matrix);
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = (u32::from(head.alpha) << 24) | 0xff_ffff;
                vertex.uv = [
                    ((head.region & 1) + x) as f32 * 0.5,
                    ((head.region >> 1) + y) as f32 * 0.5,
                ];
            }
        }
    }
}
