use super::{
    Transform,
    particle_curve::{Path, falling_points},
    particle_plugin::next_c_random,
};
use crate::kcs::{ParticleHost, ParticleVertex, ScriptRandom, transform_vector};
use std::sync::{Arc, atomic::AtomicU32};

#[derive(Clone, Copy)]
struct Sprite {
    position: [f32; 3],
    pivot: [f32; 2],
    scale: f32,
    angle: f32,
    alpha: u8,
}

#[derive(Clone, Copy)]
struct Trail {
    sprite: Sprite,
    age: Option<u8>,
}

enum Phase {
    Fall,
    Fade,
    Finished,
}

struct Spark {
    sprite: Sprite,
    path: Path,
    green: [u8; 4],
    age: i32,
    phase_start: i32,
    phase: Phase,
    trails: Vec<Trail>,
}

pub(super) struct FallSpark {
    sparks: Vec<Spark>,
    active: u32,
    image: [f32; 2],
    duration: i32,
    scale: f32,
    interval: u32,
    batch: u32,
    repeat: bool,
    module_random: Arc<AtomicU32>,
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fade(alpha: f32, first: i32, last: i32, age: i32) -> u8 {
    let (first, last, age) = (first as f32, last as f32, age as f32);
    if age <= first {
        alpha as u8
    } else if age >= last {
        0
    } else {
        (alpha - (age - first) / (last - first) * alpha) as u8
    }
}

fn spawn(
    duration: i32,
    scale: f32,
    pivot: [f32; 2],
    mut trails: Vec<Trail>,
    phase_start: i32,
    random: &mut ScriptRandom,
    module_random: &AtomicU32,
) -> Spark {
    let next = |rng: &mut ScriptRandom| f64::from(rng.next_float());
    let scale = (next(random) * f64::from(0.01f32) + f64::from(scale)) as f32;
    random.next_integer();
    let green = std::array::from_fn(|_| (random.next_integer() % 50) as u8);
    let x = (next(random) * 1280.) as f32;
    let z = (next(random) * 100.) as f32;
    let position = [x, -50., z];
    let angle = (next(random) * f64::from(std::f32::consts::PI)) as f32;
    for trail in &mut trails {
        trail.age = None;
        trail.sprite.position = [x - 40., -90., z - 5.];
        trail.sprite.alpha = 0;
    }
    let direction = [
        (next(random) * f64::from(0.4f32) - f64::from(0.2f32)) as f32,
        (next(random) * f64::from(0.8f32) + f64::from(0.2f32)) as f32,
    ];
    let points = falling_points(
        position,
        direction,
        next_c_random(module_random),
        15.,
        random,
    );
    Spark {
        sprite: Sprite {
            position,
            pivot,
            scale,
            angle,
            alpha: 0,
        },
        path: Path::new_extended_divided(points, duration as f32, 7.),
        green,
        phase: Phase::Fall,
        age: 0,
        phase_start,
        trails,
    }
}

impl FallSpark {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_random: Arc<AtomicU32>,
    ) -> Self {
        let [group, duration, scale, rate, batch, prefill, repeat, ..] =
            host.parameters.map(f32::from_bits);
        let group = (integer(f64::from(group)) as u32 as usize).max(1);
        let count = host.quads.len() / group;
        let active = if integer(f64::from(prefill)) != 0 {
            count / 2
        } else {
            0
        };
        let scale = (f64::from(scale) / 100.) as f32;
        let duration = integer(f64::from(duration));
        let pivot = image.map(|n| n * 0.5);
        let trail = Trail {
            sprite: Sprite {
                position: [0.; 3],
                pivot: [0.; 2],
                scale: 1.,
                angle: 0.,
                alpha: 0,
            },
            age: None,
        };
        let sparks = (0..count)
            .map(|i| {
                let mut spark = spawn(
                    duration,
                    scale,
                    pivot,
                    vec![trail; group - 1],
                    0,
                    random,
                    &module_random,
                );
                if i < active {
                    spark.age = integer(f64::from(random.next_float()) * f64::from(duration));
                }
                spark
            })
            .collect();
        Self {
            sparks,
            active: active as u32,
            image,
            duration,
            scale,
            interval: 61i32.wrapping_sub(integer(f64::from(rate))).clamp(1, 60) as u32,
            batch: integer(f64::from(batch)) as u32,
            repeat: integer(f64::from(repeat)) != 0,
            module_random,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        if random.next_integer().is_multiple_of(self.interval) {
            self.active = self.active.wrapping_add(self.batch);
        }
        self.active = self.active.min(self.sparks.len() as u32);
        let delta = integer(f64::from(delta as f32));
        let fall_end = (f64::from(self.duration) * f64::from(0.9f32)) as f32;
        let fall_end = integer((f64::from(fall_end) + 0.5).floor());
        for spark in &mut self.sparks[..self.active as usize] {
            let is_falling = matches!(spark.phase, Phase::Fall);
            match spark.phase {
                Phase::Fall | Phase::Fade => {
                    let [x, y, _] = spark.path.at(spark.age as f32);
                    spark.sprite.position[..2].copy_from_slice(&[x, y]);
                    spark.sprite.alpha = if is_falling {
                        190
                    } else {
                        fade(190., spark.phase_start, self.duration, spark.age)
                    };
                    spark.sprite.angle = (f64::from(spark.sprite.angle)
                        + f64::from(random.next_float()) / 10.)
                        as f32;
                    let end = if is_falling { fall_end } else { self.duration };
                    if spark.age > end {
                        spark.phase = if is_falling {
                            Phase::Fade
                        } else {
                            Phase::Finished
                        };
                        spark.phase_start = spark.age;
                    }
                }
                Phase::Finished if self.repeat => {
                    *spark = spawn(
                        self.duration,
                        self.scale,
                        spark.sprite.pivot,
                        std::mem::take(&mut spark.trails),
                        spark.phase_start,
                        random,
                        &self.module_random,
                    );
                }
                Phase::Finished => {}
            }
            if matches!(spark.phase, Phase::Fall) {
                if let Some(trail) = spark.trails.iter_mut().find(|trail| trail.age.is_none()) {
                    trail.sprite = spark.sprite;
                    trail.age = Some(0);
                }
            }
            for trail in &mut spark.trails {
                if let Some(age) = trail.age {
                    trail.sprite.position[1] = (f64::from(trail.sprite.position[1])
                        + f64::from(random.next_float()) * 0.5)
                        as f32;
                    trail.sprite.scale *= 0.99;
                    trail.age = (age < 5).then_some(age + 1);
                    trail.sprite.alpha = if trail.age.is_some() {
                        fade(100., 0, 30, i32::from(age))
                    } else {
                        0
                    };
                }
            }
            spark.age = spark.age.wrapping_add(delta);
        }
    }

    fn write_sprite(&self, sprite: Sprite, green: [u8; 4], quad: &mut [ParticleVertex; 4]) {
        let mut transform = Transform::default();
        transform.set_scale([sprite.scale, sprite.scale, 1.]);
        transform.set_rotation([0., sprite.angle, 0.]);
        let mut matrix = *transform.matrix();
        matrix[12..15].copy_from_slice(&sprite.position);
        for (i, vertex) in quad.iter_mut().enumerate() {
            let [x, y] = [(i & 1) as f32, (i >> 1) as f32];
            let point = transform_vector(
                [
                    x * self.image[0] - sprite.pivot[0],
                    y * self.image[1] - sprite.pivot[1],
                    0.,
                    1.,
                ],
                matrix,
            );
            vertex.position.copy_from_slice(&point[..3]);
            vertex.rhw = point[3];
            vertex.color =
                (u32::from(sprite.alpha) << 24) | 0x00ff_000a | (u32::from(green[i]) << 8);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        let count = self.sparks.len();
        for (i, spark) in self.sparks.iter().enumerate() {
            self.write_sprite(spark.sprite, spark.green, &mut host.quads[i]);
            for (j, trail) in spark.trails.iter().enumerate() {
                self.write_sprite(
                    trail.sprite,
                    spark.green,
                    &mut host.quads[count + i * spark.trails.len() + j],
                );
            }
        }
    }
}
