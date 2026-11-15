use super::Transform;
use crate::kcs::{ParticleHost, ScriptRandom, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fade(from: f32, to: f32, end: f32, t: f32) -> f32 {
    if t <= 0. {
        from
    } else if t >= end {
        to
    } else {
        (t / end) * (to - from) + from
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    velocity: [f32; 3],
    relative: [f32; 3],
    lives: [i32; 3],
    total: i32,
    spin: f32,
    position: [f32; 3],
    scale: f32,
    angle: f32,
    alpha: u32,
    region: u32,
}

impl Particle {
    fn spawn(scale: f32, random: &mut ScriptRandom) -> Self {
        let region = random.next_integer() & 3;
        let angle = (random.next_float() as f64 * std::f32::consts::TAU as f64) as f32;
        let spin = ((random.next_float() * 2.) - 1.) * 0.01;
        let mut velocity = [0.; 3];
        for n in &mut velocity[..2] {
            let sign = (random.next_float() as f64 * 2. - 1.) as f32;
            *n = (random.next_float() as f64 * sign as f64) as f32;
        }
        let [x, y, z] = velocity;
        let len = (((x * x + y * y) + z * z) as f64).sqrt() as f32;
        if len > 0. {
            for n in &mut velocity {
                *n /= len;
            }
        }
        let speed = (random.next_float() as f64 * 0.025f32 as f64 + 0.75) as f32;
        for n in &mut velocity {
            *n *= speed;
        }
        let lives = [
            integer(((random.next_float() * 2. - 1.) * 15.) + 30.),
            30,
            integer(((random.next_float() * 2. - 1.) * 20.) + 60.),
        ];
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            velocity,
            relative: [0.; 3],
            lives,
            total: lives.into_iter().fold(0i32, i32::wrapping_add),
            spin,
            position: [0.; 3],
            scale,
            angle,
            alpha: 0,
            region,
        }
    }
}

pub(super) struct Dark {
    anchor: [f32; 3],
    scale: f32,
    acceleration: f32,
    opacity: i32,
    center: [f32; 2],
    image: [f32; 2],
    visible: i32,
    updates: i32,
    mode: u32,
    particles: Vec<Particle>,
}

impl Dark {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2], random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let scale = p[4] / 100.;
        Self {
            anchor: [p[0], p[1], 0.],
            scale,
            acceleration: p[2] / 1000.,
            opacity: integer(p[3]),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 1,
            updates: 0,
            mode: if p[5] != 0. { 1 } else { host.render_mode },
            particles: (0..host.quads.len())
                .map(|_| Particle::spawn(scale, random))
                .collect(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2], random: &mut ScriptRandom) {
        if self.updates > 3 {
            self.visible = self
                .visible
                .wrapping_add(1)
                .min(self.particles.len() as i32);
            self.updates = 0;
        }
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let step = integer(delta as f32);
        for p in self.particles.iter_mut().take(self.visible.max(0) as usize) {
            let time = p.age.wrapping_sub(p.phase_time);
            if p.phase <= 2 {
                p.velocity[1] += self.acceleration;
                for i in 0..2 {
                    p.relative[i] += p.velocity[i];
                    p.position[i] = self.anchor[i] + p.relative[i];
                }
                p.position[2] = self.anchor[2];
                p.scale = fade(self.scale, self.scale * 2., p.total as f32, p.age as f32);
                p.angle += p.spin;
                match p.phase {
                    0 => {
                        p.alpha = integer(fade(
                            0.,
                            self.opacity as f32,
                            p.lives[0] as f32,
                            time as f32,
                        )) as u32
                            & 255;
                        if time > p.lives[0] {
                            p.phase = 1;
                            p.phase_time = p.age;
                        }
                    }
                    1 => {
                        if time > p.lives[1] {
                            p.phase = 2;
                            p.phase_time = p.age;
                        }
                    }
                    _ => {
                        p.alpha = integer(fade(
                            self.opacity as f32,
                            0.,
                            p.lives[2] as f32,
                            time as f32,
                        )) as u32
                            & 255;
                        if time > p.lives[2] {
                            p.phase = 3;
                            p.phase_time = p.age;
                        }
                    }
                }
            } else {
                *p = Particle::spawn(self.scale, random);
                p.position = self.anchor;
            }
            for (position, delta) in p.position.iter_mut().zip(offset) {
                *position += delta;
            }
            p.age = p.age.wrapping_add(step);
        }
        self.updates = self.updates.wrapping_add(1);
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., 0., p.angle]);
            let mut matrix = *transform.matrix();
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
                v.color = 0xffffff | (p.alpha << 24);
                v.uv = [
                    ((p.region & 1) + (i & 1) as u32) as f32 * 0.5,
                    ((p.region >> 1) + (i >> 1) as u32) as f32 * 0.5,
                ];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(count: usize) -> ParticleHost {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(count, [64., 128.]).unwrap();
        for (i, value) in [570f32, 220., 20., 40., 50., 0.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        host
    }

    #[test]
    fn mist_keeps_random_rounding_activation_and_accelerated_motion() {
        let mut host = host(8);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Dark::new(&host, [64., 128.], &mut random);
        let first = &effect.particles[0];
        assert_eq!(
            first.velocity.map(f32::to_bits),
            [0x3b52_d7f1, 0xbf45_f964, 0]
        );
        assert_eq!(
            (first.angle.to_bits(), first.spin.to_bits()),
            (0x406c_4413, 0xbbe5_1177)
        );
        assert_eq!(first.lives, [44, 30, 56]);
        for step in 1u32..=40 {
            let delta = if step % 9 == 0 {
                7
            } else if step % 7 == 0 {
                0
            } else {
                1
            };
            let source = [
                640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                360. + ((step % 7) as i32 - 3) as f32 * -7.5,
            ];
            effect.update(delta, source, &mut random);
            if step == 4 {
                assert_eq!(effect.visible, 1);
            }
            if step == 5 {
                assert_eq!(effect.visible, 2);
            }
        }
        let first = &effect.particles[0];
        assert_eq!(
            (first.age, first.phase_time, first.phase, first.alpha),
            (59, 45, 1, 40)
        );
        assert_eq!(first.scale.to_bits(), 0x3f39_1b92);
        assert_eq!(first.angle.to_bits(), 0x405a_5eab);
        assert_eq!(first.position, [576.62866, 190.46652, 0.]);
        assert_eq!((effect.visible, effect.updates), (8, 4));
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..8 {
            expected.next_integer();
        }
        for _ in 0..72 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        effect.write_mesh(&mut host);
        assert_eq!(
            [host.quads[0][0].uv, host.quads[0][3].uv],
            [[0., 0.], [0.5, 0.5]]
        );
        assert!(host.quads[0].iter().all(|v| v.color == 0x28ff_ffff));
    }

    #[test]
    fn strict_phase_deadlines_respawn_without_motion_and_empty_meshes_are_safe() {
        let host = host(1);
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Dark::new(&host, [64., 128.], &mut random);
        effect.particles[0].lives = [2, 3, 4];
        effect.particles[0].total = 9;
        for (age, phase, alpha) in [
            (2, 0, 40),
            (3, 1, 40),
            (6, 1, 40),
            (7, 2, 40),
            (11, 2, 0),
            (12, 3, 0),
        ] {
            effect.particles[0].age = age;
            effect.update(0, [640., 360.], &mut random);
            assert_eq!(
                (effect.particles[0].phase, effect.particles[0].alpha),
                (phase, alpha)
            );
        }
        let mut expected = random.clone();
        expected.next_integer();
        for _ in 0..9 {
            expected.next_float();
        }
        effect.update(0, [645., 367.], &mut random);
        let first = &effect.particles[0];
        assert_eq!(
            (first.age, first.phase_time, first.phase, first.alpha),
            (0, 0, 0, 0)
        );
        assert_eq!(first.position, [575., 227., 0.]);
        assert_eq!(first.relative, [0.; 3]);
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());

        let mut empty = ParticleHost::default();
        empty.reset_quads(0, [64., 128.]).unwrap();
        let mut effect = Dark::new(&empty, [64., 128.], &mut random);
        let mut expected = random.clone();
        for _ in 0..10 {
            effect.update(1, [0.; 2], &mut random);
        }
        effect.write_mesh(&mut empty);
        assert!(empty.quads.is_empty());
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
    }
}
