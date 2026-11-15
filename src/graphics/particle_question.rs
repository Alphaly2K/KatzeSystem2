use super::Transform;
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

pub(super) struct Question {
    age: i32,
    phase_time: i32,
    phase: usize,
    anchor: [f32; 2],
    center: [f32; 2],
    position: [f32; 2],
    scale: f32,
    current_scale: [f32; 2],
    angle: f32,
    initial_angle: f32,
    lives: [i32; 5],
    image: [f32; 2],
    alpha: u32,
    has_particles: bool,
}

impl Question {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let angle = if integer(p[2]) != 0 { 0.5 } else { -0.5 };
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            anchor: [p[0], p[1]],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            position: [p[0], p[1]],
            scale,
            current_scale: [scale; 2],
            angle,
            initial_angle: angle,
            lives: std::array::from_fn(|i| integer(p[i + 4])),
            image,
            alpha: 0,
            has_particles: !host.quads.is_empty(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        self.position = std::array::from_fn(|i| self.anchor[i] + (source[i] - self.center[i]));
        if !self.has_particles {
            return;
        }
        let elapsed = self.age.wrapping_sub(self.phase_time);
        match self.phase {
            0..=2 => {
                let duration = self.lives[self.phase] as f32;
                let (from, to) = match self.phase {
                    0 => ([self.scale; 2], [self.scale * 0.4, self.scale * 1.5]),
                    1 => (
                        [(self.scale as f64 * 0.4) as f32, self.scale * 1.5],
                        [self.scale * 2.4, self.scale * 0.5],
                    ),
                    _ => ([self.scale * 2.4, self.scale * 0.5], [self.scale; 2]),
                };
                self.current_scale =
                    std::array::from_fn(|i| fade(from[i], to[i], duration, elapsed as f32));
                if self.phase < 2 {
                    let (from, to) = if self.phase == 0 {
                        (0., 128.)
                    } else {
                        (128., 255.)
                    };
                    self.alpha = integer(fade(from, to, duration, elapsed as f32)) as u32 & 255;
                }
                if elapsed > self.lives[self.phase] {
                    self.phase += 1;
                    self.phase_time = self.age;
                }
            }
            3 => {
                if elapsed > self.lives[3] {
                    self.phase = 4;
                    self.phase_time = self.age;
                }
            }
            4 => {
                let angle = (std::f32::consts::TAU / self.lives[4] as f32) * elapsed as f32;
                let cosine = if angle.is_finite() {
                    (angle as f64).cos() as f32
                } else {
                    f32::from_bits(0xffc0_0000)
                };
                self.angle += cosine / 50.;
                if elapsed > self.lives[4] {
                    self.phase = 3;
                    self.angle = self.initial_angle;
                    self.phase_time = self.age;
                }
            }
            _ => unreachable!(),
        }
        self.age = self.age.wrapping_add(integer(delta as f32));
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let mut transform = Transform::default();
        transform.set_scale([self.current_scale[0], self.current_scale[1], 1.]);
        transform.set_rotation([0., 0., self.angle]);
        let mut matrix = *transform.matrix();
        matrix[12..14].copy_from_slice(&self.position);
        let half = self.image.map(|n| n * 0.5);
        for quad in &mut host.quads {
            for (i, vertex) in quad.iter_mut().enumerate() {
                let y = match i {
                    0 | 1 => -half[1],
                    // 第三个角点的下沿沿用半宽。
                    2 => half[0],
                    _ => half[1],
                };
                let point = transform_vector(
                    [
                        if i & 1 == 0 { -half[0] } else { half[0] },
                        y - half[1],
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0xffffff | (self.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn question_keeps_stretch_boundaries_bottom_pivot_and_sway_cycles() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [719f32, 209., 1., 60., 5., 8., 10., 60., 80.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Question::new(&host, [64., 128.]);
        for step in 1u32..160 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(
                delta,
                [
                    640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                    360. + ((step % 7) as i32 - 3) as f32 * -7.5,
                ],
            );
            let expected = match step {
                7 => Some([6, 6, 1, 0x3f00_0000, 0x3e75_c290, 0x3f66_6667, 128]),
                11 => Some([16, 15, 2, 0x3f00_0000, 0x3fb8_51ec, 0x3e99_999a, 255]),
                19 => Some([29, 28, 3, 0x3f00_0000, 0x3f19_999a, 0x3f19_999a, 255]),
                62 => Some([90, 89, 4, 0x3f00_0000, 0x3f19_999a, 0x3f19_999a, 255]),
                63 => Some([97, 89, 4, 0x3f05_1aae, 0x3f19_999a, 0x3f19_999a, 255]),
                114 => Some([171, 170, 3, 0x3f00_0000, 0x3f19_999a, 0x3f19_999a, 255]),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(
                    [
                        effect.age as u32,
                        effect.phase_time as u32,
                        effect.phase as u32,
                        effect.angle.to_bits(),
                        effect.current_scale[0].to_bits(),
                        effect.current_scale[1].to_bits(),
                        effect.alpha,
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 63 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let quad = mesh.projected_quad(0, &Projection::default()).unwrap();
                assert_eq!(
                    [
                        quad[0].position[0].to_bits(),
                        quad[0].position[1].to_bits(),
                        quad[2].position[0].to_bits(),
                        quad[2].position[1].to_bits(),
                    ],
                    [0x443b_8f97, 0x431b_4f60, 0x4434_6816, 0x434d_4c58]
                );
            }
        }

        host.parameters = [0; 32];
        let mut zero_period = Question::new(&host, [64., 128.]);
        assert_eq!(
            (zero_period.scale, zero_period.initial_angle),
            (0.001, -0.5)
        );
        for _ in 0..4 {
            zero_period.update(1, [640., 360.]);
        }
        zero_period.update(0, [640., 360.]);
        zero_period.update(0, [640., 360.]);
        assert_eq!(
            (zero_period.phase, zero_period.angle.to_bits()),
            (4, 0xffc0_0000)
        );

        host.reset_quads(0, [64., 128.]).unwrap();
        let mut empty = Question::new(&host, [64., 128.]);
        empty.update(7, [650., 380.]);
        assert_eq!((empty.age, empty.position), (0, [10., 20.]));
    }
}
