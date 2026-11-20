use super::{Transform, particle_curve::Path};
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

#[derive(Clone, Copy)]
enum Phase {
    FadeIn,
    Travel,
    FadeOut,
    Finished,
}

struct Particle {
    path: Path,
    position: [f32; 3],
    scale: f32,
    angle: f32,
    age: i32,
    phase_start: i32,
    phase: Phase,
    alpha: u8,
}

pub(super) struct Erase {
    image: [f32; 2],
    durations: [i32; 3],
    particles: Vec<Particle>,
}

fn truncate(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn lerp(start: f32, end: f32, duration: f32, time: f32) -> f32 {
    if time <= 0. {
        start
    } else if time >= duration {
        end
    } else {
        (time / duration) * (end - start) + start
    }
}

fn normalized([x, y]: [f32; 2]) -> [f32; 2] {
    let len = ((f64::from(x) * f64::from(x) + f64::from(y) * f64::from(y)) as f32).sqrt();
    if len > 0. { [x / len, y / len] } else { [x, y] }
}

fn spawn(
    guides: &[Path; 2],
    scale: f32,
    distance: f32,
    duration: i32,
    strength: f32,
    random: &mut ScriptRandom,
) -> Particle {
    let r = |random: &mut ScriptRandom| f64::from(random.next_float());
    let scale = (r(random) * f64::from(scale) + f64::from(scale)) as f32;
    let time = (r(random) * 100.) as f32;
    let [start, end] = guides.each_ref().map(|path| path.at(time));
    let x = lerp(start[0], end[0], 100., (r(random) * 100.) as f32);
    let y = lerp(start[1], end[1], 100., (r(random) * 100.) as f32);
    let position = [x, y, (r(random) * 50.) as f32];
    let angle = (r(random) * f64::from(std::f32::consts::PI)) as f32;
    let jitter = normalized(std::array::from_fn(|_| (r(random) * 30. - 15.) as f32));
    let step =
        |random: &mut ScriptRandom| (r(random) * f64::from(distance) + f64::from(distance)) as f32;
    let first_step = step(random);
    let mut point = [
        x + jitter[0] * first_step,
        y + jitter[1] * first_step,
        position[2],
    ];
    let mut points = vec![position, point];
    let direction = normalized([point[0] - x, point[1] - y]);
    let [left, right] = [-20f64, 20.].map(|degrees| {
        let angle = (degrees * f64::from(std::f32::consts::PI) / 180.) as f32;
        let (sin, cos) = f64::from(angle).sin_cos();
        let (sin, cos) = (f64::from(sin as f32), f64::from(cos as f32));
        let [x, y] = direction.map(f64::from);
        [(x * cos - y * sin) as f32, (x * sin + y * cos) as f32]
    });
    for _ in 0..4 {
        let drift = std::array::from_fn(|i| {
            let span = (left[i] - right[i]).abs();
            let offset = (r(random) * f64::from(span) - f64::from(span) * 0.5) as f32;
            direction[i] + offset
        });
        let [dx, dy] = normalized(drift);
        let distance = step(random);
        point = [point[0] + dx * distance, point[1] + dy * distance, point[2]];
        points.push(point);
        if random.next_integer().is_multiple_of(15) {
            break;
        }
    }
    Particle {
        path: Path::new_extended(points, duration as f32, strength),
        position,
        scale,
        angle,
        age: 0,
        phase_start: 0,
        phase: Phase::FadeIn,
        alpha: 0,
    }
}

impl Erase {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let [
            _,
            ax,
            ay,
            bx,
            by,
            cx,
            cy,
            dx,
            dy,
            ex,
            ey,
            fx,
            fy,
            scale,
            distance,
            rise,
            travel,
            fade,
            strength,
            stagger,
            ..,
        ] = host.parameters.map(f32::from_bits);
        let guides = [
            Path::new_extended(
                vec![[ax, ay, 0.], [bx, by, 0.], [cx, cy, 0.]],
                100.,
                strength,
            ),
            Path::new_extended(vec![[dx, dy, 0.], [ex, ey, 0.], [fx, fy, 0.]], 100., 100.),
        ];
        let durations = [rise, travel, fade].map(|n| truncate(f64::from(n)));
        let duration = durations.into_iter().fold(0, i32::wrapping_add);
        let count = host.quads.len();
        let prefill = if truncate(f64::from(stagger)) != 0 {
            count / 2
        } else {
            0
        };
        let scale = (f64::from(scale) / 100.) as f32;
        let particles = (0..count)
            .map(|i| {
                let mut particle = spawn(&guides, scale, distance, duration, strength, random);
                if i < prefill {
                    particle.age = truncate(f64::from(random.next_float()) * f64::from(duration));
                }
                particle
            })
            .collect();
        Self {
            image,
            durations,
            particles,
        }
    }

    pub(super) fn update(&mut self, delta: u32) {
        let delta = truncate(f64::from(delta as f32));
        for p in &mut self.particles {
            let elapsed = p.age.wrapping_sub(p.phase_start);
            let next = match p.phase {
                Phase::FadeIn => {
                    let rise = self.durations[0];
                    p.alpha = lerp(0., 128., rise as f32, p.age as f32) as u8;
                    (p.age > rise).then_some(Phase::Travel)
                }
                Phase::Travel => (elapsed > self.durations[1]).then_some(Phase::FadeOut),
                Phase::FadeOut => {
                    let fade = self.durations[2];
                    p.alpha = lerp(128., 0., fade as f32, elapsed as f32) as u8;
                    (elapsed > fade).then_some(Phase::Finished)
                }
                Phase::Finished => None,
            };
            if !matches!(p.phase, Phase::Finished) {
                p.position = p.path.at(p.age as f32);
            }
            if let Some(next) = next {
                p.phase = next;
                p.phase_start = p.age;
            }
            p.age = p.age.wrapping_add(delta);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., 0., p.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, vertex) in quad.iter_mut().enumerate() {
                let x = (i & 1) as f32;
                let y = (i >> 1) as f32;
                let point = transform_vector(
                    [(x - 0.5) * self.image[0], (y - 0.5) * self.image[1], 0., 1.],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = (u32::from(p.alpha) << 24) | 0xff_ffff;
                vertex.uv = [x, y];
            }
        }
    }
}
