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
    rotation: [f32; 3],
    scale: f32,
    pivot: [f32; 2],
    atlas: [f32; 2],
    alpha: u8,
}

#[derive(Clone, Copy)]
struct Trail {
    sprite: Sprite,
    active: bool,
}

struct Particle {
    sprite: Sprite,
    path: Path,
    trails: Vec<Trail>,
    age: i32,
    phase_start: i32,
    phase: u8,
}

pub(super) struct RadialIce {
    particles: Vec<Particle>,
    active: usize,
    image: [f32; 2],
    mode: u32,
    module_random: Arc<AtomicU32>,
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn opacity(first: f32, last: f32, start: f32, end: f32, time: f32) -> u8 {
    let value = if time <= start {
        first
    } else if time >= end {
        last
    } else {
        (time - start) / (end - start) * (last - first) + first
    };
    integer(f64::from(value)) as u8
}

fn spawn(
    p: &[f32; 32],
    pivot: [f32; 2],
    mut trails: Vec<Trail>,
    phase_start: i32,
    random: &mut ScriptRandom,
    module_random: &AtomicU32,
) -> Particle {
    let next = |rng: &mut ScriptRandom| f64::from(rng.next_float());
    let atlas = [(random.next_integer() & 1) as f32 * 0.5, 0.];
    let scale = (next(random) * f64::from(0.01f32) + f64::from(p[2]) / 100.) as f32;
    random.next_integer();
    let x = (next(random) * 1280.) as f32;
    let raw_z = (next(random) * 100.) as f32;
    let z_adjust = f64::from(p[5]) / 100.;
    let position = [x, -50., (f64::from(raw_z) + z_adjust) as f32];
    let rotation = [
        0.,
        (next(random) * f64::from(std::f32::consts::PI)) as f32,
        0.,
    ];
    let sprite = Sprite {
        position,
        rotation,
        scale,
        pivot,
        atlas,
        alpha: 0,
    };
    for trail in &mut trails {
        trail.active = false;
        trail.sprite.position = [x - 40., -90., (f64::from(position[2]) - z_adjust) as f32];
        trail.sprite.alpha = 0;
    }

    let direction = [(next(random) - 0.5) as f32, random.next_float()];
    let points = falling_points(
        position,
        direction,
        next_c_random(module_random),
        10.,
        random,
    );
    Particle {
        sprite,
        path: Path::new_extended(points, p[1], 50.),
        trails,
        age: 0,
        phase_start,
        phase: 0,
    }
}

impl RadialIce {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
        module_random: Arc<AtomicU32>,
    ) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let group = (integer(f64::from(p[0])) as u32 as usize).max(1);
        let count = host.quads.len() / group;
        let prefill = if integer(f64::from(p[3])) == 0 {
            0
        } else {
            count / 2
        };
        let pivot = image.map(|n| n * 0.5);
        let trail = Trail {
            sprite: Sprite {
                position: [0.; 3],
                rotation: [0.; 3],
                scale: 1.,
                pivot: [0.; 2],
                atlas: [0., 0.5],
                alpha: 0,
            },
            active: false,
        };
        let particles = (0..count)
            .map(|i| {
                let mut particle =
                    spawn(&p, pivot, vec![trail; group - 1], 0, random, &module_random);
                if i < prefill {
                    particle.age = integer(f64::from(random.next_float()) * f64::from(p[1]));
                }
                particle
            })
            .collect();
        Self {
            particles,
            active: prefill,
            image,
            mode: u32::from(integer(f64::from(p[6])) == 0),
            module_random,
        }
    }

    pub(super) fn update(&mut self, delta: u32, parameters: &[u32; 32], random: &mut ScriptRandom) {
        let p = parameters.map(f32::from_bits);
        if random.next_integer().is_multiple_of(10) {
            self.active = (self.active + 2).min(self.particles.len());
        }
        let limit = (f64::from(p[1]) * f64::from(0.9f32)) as f32;
        let limit = integer((f64::from(limit) + 0.5).floor());
        let delta = integer(f64::from(delta as f32));
        for particle in &mut self.particles[..self.active] {
            let s = &mut particle.sprite;
            let age = particle.age;
            let elapsed = age.wrapping_sub(particle.phase_start) as f32;
            let mut advance_phase = false;
            let turn = |s: &mut Sprite, random: &mut ScriptRandom| {
                s.rotation[1] =
                    (f64::from(s.rotation[1]) + f64::from(random.next_float()) / 10.) as f32;
            };
            let jitter = |s: &mut Sprite, random: &mut ScriptRandom| {
                for coordinate in &mut s.position[..2] {
                    *coordinate =
                        (f64::from(*coordinate) + f64::from(random.next_float()) * 10. - 5.) as f32;
                }
            };
            match particle.phase {
                0 | 1 => {
                    s.position[..2].copy_from_slice(&particle.path.at(age as f32)[..2]);
                    s.alpha = if particle.phase == 0 {
                        64
                    } else {
                        opacity(64., 0., particle.phase_start as f32, p[1], age as f32)
                    };
                    turn(s, random);
                    advance_phase = if particle.phase == 0 {
                        age > limit
                    } else {
                        age as f32 > p[1]
                    };
                    if advance_phase && particle.phase == 1 {
                        s.scale *= 1.5;
                        s.rotation = [
                            0.,
                            0.,
                            (f64::from(random.next_float()) * f64::from(std::f32::consts::TAU))
                                as f32,
                        ];
                        s.atlas = [0.5, 0.5];
                        jitter(s, random);
                    }
                }
                2 | 3 => {
                    let duration = if particle.phase == 2 { 5. } else { 10. };
                    s.alpha = opacity(0., 255., 0., duration, elapsed);
                    s.position[1] =
                        (f64::from(s.position[1]) + f64::from(random.next_float())) as f32;
                    advance_phase = elapsed > duration;
                    if advance_phase && particle.phase == 2 {
                        jitter(s, random);
                    }
                }
                4 => {
                    s.alpha = opacity(255., 0., 0., 20., elapsed);
                    advance_phase = elapsed > 20.;
                }
                _ if integer(f64::from(p[4])) != 0 => {
                    *particle = spawn(
                        &p,
                        s.pivot,
                        std::mem::take(&mut particle.trails),
                        particle.phase_start,
                        random,
                        &self.module_random,
                    );
                }
                _ => {}
            }
            if advance_phase {
                particle.phase += 1;
                particle.phase_start = age;
            }
            if particle.phase == 0 {
                if let Some(trail) = particle.trails.iter_mut().find(|trail| !trail.active) {
                    trail.active = true;
                    trail.sprite.pivot = particle.sprite.pivot;
                    trail.sprite.scale = particle.sprite.scale * 1.5;
                }
            }
            for trail in &mut particle.trails {
                if trail.active {
                    trail.sprite.position[..2].copy_from_slice(&particle.sprite.position[..2]);
                    trail.active = particle.phase <= 1;
                    trail.sprite.alpha = if trail.active { 32 } else { 0 };
                }
            }
            particle.age = particle.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        let count = self.particles.len();
        for (i, particle) in self.particles.iter().enumerate() {
            self.write_sprite(particle.sprite, &mut host.quads[i]);
            for (j, trail) in particle.trails.iter().enumerate() {
                self.write_sprite(
                    trail.sprite,
                    &mut host.quads[count + i * particle.trails.len() + j],
                );
            }
        }
    }

    fn write_sprite(&self, sprite: Sprite, quad: &mut [ParticleVertex; 4]) {
        let mut transform = Transform::default();
        transform.set_scale([sprite.scale, sprite.scale, 1.]);
        transform.set_rotation(sprite.rotation);
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
            vertex.color = 0xff_ffff | (u32::from(sprite.alpha) << 24);
            vertex.uv = [sprite.atlas[0] + x * 0.5, sprite.atlas[1] + y * 0.5];
        }
    }
}
