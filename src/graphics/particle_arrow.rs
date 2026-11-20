use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

const TRAIL_ATLAS: [[f32; 2]; 4] = [[0., 0.5], [0.25, 0.5], [0., 0.75], [0.25, 0.75]];

struct Trail {
    age: i32,
    active: bool,
    path: Option<Path>,
    position: [f32; 3],
    scale: f32,
    alpha: u8,
    atlas: [f32; 2],
}

struct Head {
    age: i32,
    phase_start: i32,
    phase: u32,
    position: [f32; 3],
    trails: Vec<Trail>,
}

pub(super) struct Arrow {
    image: [f32; 2],
    start: [f32; 2],
    end: [f32; 2],
    scale: f32,
    angle: f32,
    duration: i32,
    heads: Vec<Head>,
    module_state: Arc<AtomicU32>,
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&f64::from(value)) {
        value as i32
    } else {
        i32::MIN
    }
}

fn linear(a: f32, b: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        a
    } else if time >= duration {
        b
    } else {
        (time / duration) * (b - a) + a
    }
}

fn head_angle(start: [f32; 2], end: [f32; 2]) -> f32 {
    let [x, y] = std::array::from_fn(|i| end[i] - start[i]);
    let len = ((f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32).sqrt();
    let axis = if len > 0. { y / len } else { y };
    let angle = f64::from(axis).acos() as f32;
    (f64::from(std::f32::consts::FRAC_PI_2) + f64::from(if x > 0. { -angle } else { angle })) as f32
}

impl Arrow {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_state: Arc<AtomicU32>,
    ) -> Self {
        let [group, x, y, end_x, end_y, scale, duration, ..] = host.parameters.map(f32::from_bits);
        let group = (integer(group) as u32 as usize).max(1);
        let start = [x, y];
        let end = [end_x, end_y];
        let heads = (0..host.quads.len() / group)
            .map(|_| Head {
                age: 0,
                phase_start: 0,
                phase: 0,
                position: [x, y, 0.],
                trails: (1..group)
                    .map(|_| Trail {
                        age: 0,
                        active: false,
                        path: None,
                        position: [0.; 3],
                        scale: 1.,
                        alpha: 0,
                        atlas: TRAIL_ATLAS[(random.next_integer() & 3) as usize],
                    })
                    .collect(),
            })
            .collect();
        Self {
            image,
            start,
            end,
            scale: scale / 100.,
            angle: head_angle(start, end),
            duration: integer(duration),
            heads,
            module_state,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let increment = u32::from(random.next_integer().is_multiple_of(10));
        let count = self.heads.len() as u32;
        let previous = self
            .module_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some((n + increment).min(count))
            })
            .unwrap();
        let delta = integer(delta as f32);
        for head in &mut self.heads[..(previous + increment).min(count) as usize] {
            if head.phase == 0 {
                for i in 0..2 {
                    head.position[i] = linear(
                        self.start[i],
                        self.end[i],
                        self.duration as f32,
                        head.age as f32,
                    );
                }
                if head.age > self.duration {
                    head.phase = 1;
                    head.phase_start = head.age;
                }
            } else if head.phase == 1 && head.age > 60 {
                head.phase = 2;
                head.phase_start = head.age;
            }

            let duration = head.trails.len() as f32;
            if head.phase == 0 {
                for trail in head.trails.iter_mut().filter(|trail| !trail.active) {
                    let mut point = head.position;
                    let mut points = vec![point];
                    for step in [[60., 20.], [80., 80.], [80., 80.]] {
                        for i in 0..2 {
                            point[i] = (f64::from(random.next_float()) * step[i]
                                + f64::from(point[i]))
                                as f32;
                        }
                        points.push(point);
                    }
                    trail.path = Some(Path::new_extended(points, duration, 20.));
                    trail.scale = self.scale * 0.1;
                    trail.active = true;
                    // The original samples a spin for the head without applying it.
                    random.next_float();
                    if random.next_integer().is_multiple_of(5) {
                        break;
                    }
                }
            }
            for trail in head.trails.iter_mut().filter(|trail| trail.active) {
                trail.position = trail.path.as_ref().unwrap().at(trail.age as f32);
                trail.scale *= 0.99;
                trail.alpha = integer(linear(255., 0., duration, trail.age as f32)) as u8;
                trail.age = trail.age.wrapping_add(delta);
                if trail.age >= duration as i32 {
                    trail.age = 0;
                    trail.active = false;
                    trail.alpha = 0;
                }
            }
            head.age = head.age.wrapping_add(delta);
        }
    }

    fn write_quad(
        &self,
        quad: &mut [crate::kcs::ParticleVertex; 4],
        position: [f32; 3],
        scale: [f32; 2],
        angle: f32,
        alpha: u8,
        uv: [f32; 4],
    ) {
        let mut transform = Transform::default();
        transform.set_scale([scale[0], scale[1], 1.]);
        transform.set_rotation([0., 0., angle]);
        let mut matrix = *transform.matrix();
        matrix[12..15].copy_from_slice(&position);
        let [u, v, width, height] = uv;
        for (i, vertex) in quad.iter_mut().enumerate() {
            let x = (i & 1) as f32;
            let y = (i >> 1) as f32;
            let point = transform_vector(
                [(x - 0.5) * self.image[0], (y - 0.5) * self.image[1], 0., 1.],
                matrix,
            );
            vertex.position.copy_from_slice(&point[..3]);
            vertex.rhw = point[3];
            vertex.color = (u32::from(alpha) << 24) | 0xff_ffff;
            vertex.uv = [u + x * width, v + y * height];
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let count = self.heads.len();
        for (i, head) in self.heads.iter().enumerate() {
            self.write_quad(
                &mut host.quads[i],
                head.position,
                [self.scale * 2., self.scale * 0.1],
                self.angle,
                128,
                [0., 0., 1., 0.5],
            );
            for (j, trail) in head.trails.iter().enumerate() {
                let [u, v] = trail.atlas;
                let index = count + i * head.trails.len() + j;
                self.write_quad(
                    &mut host.quads[index],
                    trail.position,
                    [trail.scale; 2],
                    0.,
                    trail.alpha,
                    [u, v, 0.25, 0.25],
                );
            }
        }
    }
}
