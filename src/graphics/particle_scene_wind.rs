use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ParticleVertex, ScriptRandom, transform_vector};

#[derive(Clone, Copy)]
struct Sprite {
    position: [f32; 3],
    pivot: [f32; 2],
    scale: [f32; 2],
    angle: f32,
}

struct Trail {
    sprite: Sprite,
    age: Option<i32>,
    alpha: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrailEmission {
    Waiting,
    Emitting,
    Full,
}

struct Head {
    sprite: Sprite,
    previous: [f32; 3],
    path: Path,
    retired: bool,
    age: i32,
    trails: Vec<Trail>,
    emission: TrailEmission,
}

struct Config {
    guide: Path,
    image: [f32; 2],
    scale: f32,
    step: f32,
    ages: [i32; 3],
    strength: f32,
    x_shift: f32,
    x_range: f32,
    y_mode: i32,
    y_range: f32,
}

pub(super) struct SceneWind {
    config: Config,
    heads: Vec<Head>,
    active: usize,
}

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn normalize([x, y]: [f32; 2]) -> [f32; 2] {
    let length = ((f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32).sqrt();
    if length > 0. {
        [x / length, y / length]
    } else {
        [x, y]
    }
}

impl Config {
    fn spawn(&self, mut trails: Vec<Trail>, random: &mut ScriptRandom) -> Head {
        let next = |random: &mut ScriptRandom| f64::from(random.next_float());
        let scale = (f64::from(self.scale) * (next(random) + 1.)) as f32;
        let start = self.guide.at((next(random) * 100.) as f32);
        let angle = (next(random) * f64::from(std::f32::consts::PI)) as f32;
        let age = integer(next(random) * f64::from(self.ages[0].wrapping_add(self.ages[1])));
        let dx = next(random) * f64::from(self.x_range) - f64::from(self.x_shift);
        let random_y = next(random) * f64::from(self.y_range);
        let offset = f64::from(0.05f32);
        let dy = match self.y_mode {
            1 => -(random_y + offset),
            2 => random_y - f64::from(self.y_range) * 0.5,
            _ => random_y + offset,
        };
        let [x, y] = normalize([dx as f32, dy as f32]);
        let distance =
            |random: &mut ScriptRandom| (f64::from(self.step) * (next(random) + 1.)) as f32;
        let step = distance(random);
        let mut point = [start[0] + x * step, start[1] + y * step, start[2]];
        let [dx, dy] = normalize([point[0] - start[0], point[1] - start[1]]);
        let turn = (50. * f64::from(std::f32::consts::PI) / 180.) as f32;
        let (sin, cos) = f64::from(turn).sin_cos();
        let (sin, cos) = (f64::from(sin as f32), f64::from(cos as f32));
        let [x, y] = [dx, dy].map(f64::from);
        let x_span = ((x * cos + y * sin) as f32 - (x * cos - y * sin) as f32).abs();
        let y_span = ((y * cos - x * sin) as f32 - (y * cos + x * sin) as f32).abs();
        let mut points = vec![start, point];
        for _ in 0..4 {
            let jitter = |span: f32, random: &mut ScriptRandom| {
                (f64::from(span) * (next(random) - 0.5)) as f32
            };
            let direction = normalize([dx + jitter(x_span, random), dy + jitter(y_span, random)]);
            let step = distance(random);
            point[0] += direction[0] * step;
            point[1] += direction[1] * step;
            points.push(point);
        }
        let duration = self.ages.into_iter().fold(0, i32::wrapping_add);
        let path = Path::new_extended(points, duration as f32, self.strength);
        let position = path.at(age as f32);
        for trail in &mut trails {
            trail.age = None;
            trail.alpha = 0;
            trail.sprite.position = position;
        }
        Head {
            sprite: Sprite {
                position,
                angle,
                scale: [scale; 2],
                pivot: self.image.map(|n| n * 0.5),
            },
            previous: start,
            path,
            retired: false,
            age,
            trails,
            emission: TrailEmission::Waiting,
        }
    }
}

impl Head {
    fn update(&mut self, config: &Config, delta: i32, random: &mut ScriptRandom) {
        if self.retired {
            if f64::from(self.age) > f64::from(self.path.duration) + 60. {
                let emission = self.emission;
                *self = config.spawn(std::mem::take(&mut self.trails), random);
                self.emission = emission;
            }
        } else {
            self.sprite.position = self.path.at(self.age as f32);
            if self.emission == TrailEmission::Waiting && random.next_integer().is_multiple_of(40) {
                self.emission = TrailEmission::Emitting;
            }
            if self.age as f32 > self.path.duration {
                self.retired = true;
            }
        }
        if self.emission == TrailEmission::Emitting {
            let count = self.trails.len();
            if let Some((i, trail)) = self
                .trails
                .iter_mut()
                .enumerate()
                .find(|(_, t)| t.age.is_none())
            {
                trail.activate(self.sprite, self.previous);
                if i + 1 == count {
                    self.emission = TrailEmission::Full;
                }
            }
        }
        for (i, trail) in self.trails.iter_mut().enumerate() {
            if let Some(age) = trail.age {
                trail.alpha = if age <= 30 {
                    255
                } else if age >= 60 {
                    0
                } else {
                    (((age as f32 - 30.) / 30.) * -255. + 255.) as u8
                };
                let age = age.wrapping_add(delta);
                if age >= 60 {
                    trail.age = None;
                    trail.alpha = 0;
                    if i == 0 {
                        self.emission = TrailEmission::Waiting;
                    }
                } else {
                    trail.age = Some(age);
                }
            }
        }
        self.previous = self.sprite.position;
        self.age = self.age.wrapping_add(delta);
    }
}

impl Trail {
    fn activate(&mut self, sprite: Sprite, previous: [f32; 3]) {
        let mut pose = self.sprite;
        pose.position = sprite.position;
        pose.pivot = sprite.pivot;
        pose.scale = sprite.scale;
        let [x, y, z] = std::array::from_fn(|i| sprite.position[i] - previous[i]);
        let [xx, yy, zz] = [x, y, z].map(f64::from);
        let length = (((xx * xx + yy * yy) + zz * zz) as f32).sqrt();
        if length > 0. {
            pose.angle = f64::from(y / length).acos() as f32;
            if x > 0. {
                pose.angle = -pose.angle;
            }
            pose.position = std::array::from_fn(|i| {
                ((f64::from(sprite.position[i]) + f64::from(previous[i])) * 0.5) as f32
            });
            let length = (x * x + y * y + z * z).sqrt();
            pose.scale[1] =
                (f64::from(length) * f64::from(0.7f32) * f64::from(sprite.scale[1])) as f32;
        }
        self.sprite = pose;
        self.age = Some(0);
    }
}

impl SceneWind {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let [
            group,
            x,
            y,
            end_x,
            end_y,
            scale,
            step,
            rise,
            travel,
            fade,
            strength,
            shift,
            x_range,
            y_mode,
            y_range,
            ..,
        ] = host.parameters.map(f32::from_bits);
        let percent = |n: f32| (f64::from(n) / 100.) as f32;
        let config = Config {
            guide: Path::new_extended(vec![[x, y, 0.], [end_x, end_y, 0.]], 100., 1.),
            image,
            scale: percent(scale),
            step,
            ages: [rise, travel, fade].map(|n| integer(f64::from(n))),
            strength,
            x_shift: percent(shift),
            x_range: percent(x_range),
            y_mode: integer(f64::from(y_mode)),
            y_range: percent(y_range),
        };
        let group = (integer(f64::from(group)) as u32 as usize).max(1);
        let count = host.quads.len() / group;
        let heads = (0..count)
            .map(|_| {
                let trails = (1..group)
                    .map(|_| Trail {
                        sprite: Sprite {
                            position: [0.; 3],
                            pivot: [0.; 2],
                            scale: [1.; 2],
                            angle: 0.,
                        },
                        age: None,
                        alpha: 0,
                    })
                    .collect();
                config.spawn(trails, random)
            })
            .collect();
        Self {
            config,
            heads,
            active: 1,
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        self.active = (self.active + usize::from(random.next_integer().is_multiple_of(10)))
            .min(self.heads.len());
        let delta = integer(f64::from(delta as f32));
        for head in &mut self.heads[..self.active] {
            head.update(&self.config, delta, random);
        }
    }

    fn write(&self, sprite: Sprite, quad: &mut [ParticleVertex; 4], color: u32) {
        let mut transform = Transform::default();
        transform.set_scale([sprite.scale[0], sprite.scale[1], 1.]);
        transform.set_rotation([0., 0., sprite.angle]);
        let mut matrix = *transform.matrix();
        matrix[12..15].copy_from_slice(&sprite.position);
        for (i, vertex) in quad.iter_mut().enumerate() {
            let [x, y] = [(i & 1) as f32, (i >> 1) as f32];
            let point = transform_vector(
                [
                    x * self.config.image[0] - sprite.pivot[0],
                    y * self.config.image[1] - sprite.pivot[1],
                    0.,
                    1.,
                ],
                matrix,
            );
            vertex.position.copy_from_slice(&point[..3]);
            vertex.rhw = point[3];
            vertex.color = color;
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 1;
        let count = self.heads.len();
        for (i, head) in self.heads.iter().enumerate() {
            self.write(head.sprite, &mut host.quads[i], 0x00ff_0000);
            for (j, trail) in head.trails.iter().enumerate() {
                let color = (u32::from(trail.alpha) << 24) | 0x00ff_1e1e;
                self.write(
                    trail.sprite,
                    &mut host.quads[count + i * head.trails.len() + j],
                    color,
                );
            }
        }
    }
}
