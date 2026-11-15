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

pub(super) struct Exclamation {
    age: i32,
    phase_time: i32,
    phase: u32,
    anchor: [f32; 2],
    endpoints: [[f32; 2]; 2],
    center: [f32; 2],
    position: [f32; 2],
    scale: f32,
    current_scale: [f32; 2],
    angle: f32,
    tilt: f32,
    lives: [i32; 5],
    image: [f32; 2],
    alpha: u32,
    has_particles: bool,
}

impl Exclamation {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1]];
        let angle = if integer(p[2]) != 0 { 0.4f32 } else { -0.4 };
        let quantized = integer(angle * 1000.) as f32 / 1000.;
        let cosine = (quantized as f64).cos() as f32;
        let sine = (quantized as f64).sin() as f32;
        let x = ((cosine * 0. - (-sine * 1.)) + 0.) + 0.;
        let y = ((x * sine - cosine * 1.) + 0.) + 0.;
        let endpoints =
            [100., 40.].map(|distance| std::array::from_fn(|i| [x, y][i] * distance + anchor[i]));
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            anchor,
            endpoints,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            position: anchor,
            scale,
            current_scale: [scale; 2],
            angle,
            tilt: 0.,
            lives: std::array::from_fn(|i| integer(p[i + 4])),
            image,
            alpha: 0,
            has_particles: !host.quads.is_empty(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if !self.has_particles {
            return;
        }
        let elapsed = self.age.wrapping_sub(self.phase_time);
        match self.phase {
            0..=2 => {
                let (from, to, position_duration) = match self.phase {
                    0 => (self.anchor, self.endpoints[0], self.lives[0]),
                    1 => (self.endpoints[0], self.anchor, self.lives[1]),
                    _ => (self.anchor, self.endpoints[1], self.lives[0]),
                };
                self.position = std::array::from_fn(|i| {
                    fade(from[i], to[i], position_duration as f32, elapsed as f32)
                });
                let duration = self.lives[self.phase as usize] as f32;
                let (from, to) = match self.phase {
                    0 => ([self.scale; 2], [self.scale * 0.6, self.scale * 1.5]),
                    1 => (
                        [(self.scale as f64 * 0.6) as f32, self.scale * 1.5],
                        [self.scale * 1.8, self.scale * 0.5],
                    ),
                    _ => ([self.scale * 1.8, self.scale * 0.5], [self.scale; 2]),
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
                if elapsed > self.lives[self.phase as usize] {
                    self.phase += 1;
                    self.phase_time = self.age;
                }
            }
            3 => {
                self.position = self.endpoints[1];
                if elapsed > self.lives[4] {
                    self.phase = 4;
                    self.phase_time = self.age;
                }
            }
            4 => {
                self.position = self.endpoints[1];
                let angle = (std::f32::consts::PI / self.lives[4] as f32) * elapsed as f32;
                let sine = if angle.is_finite() {
                    (angle as f64).sin() as f32
                } else {
                    f32::from_bits(0xffc0_0000)
                };
                self.tilt += sine * 0.5;
                if elapsed > self.lives[3] {
                    self.phase = 3;
                    self.tilt = 0.;
                    self.phase_time = self.age;
                }
            }
            _ => unreachable!(),
        }
        for (i, n) in source.into_iter().enumerate() {
            self.position[i] += n - self.center[i];
        }
        self.age = self.age.wrapping_add(integer(delta as f32));
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let mut transform = Transform::default();
        transform.set_scale([self.current_scale[0], self.current_scale[1], 1.]);
        transform.set_rotation([0., self.tilt, self.angle]);
        let mut matrix = *transform.matrix();
        matrix[12..14].copy_from_slice(&self.position);
        for quad in &mut host.quads {
            for (i, vertex) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        if i & 1 == 0 {
                            -self.image[0] * 0.5
                        } else {
                            self.image[0] * 0.5
                        },
                        match i {
                            0 | 1 => -self.image[1] * 0.5,
                            // 第三个角点的下沿保留原插件的半宽。
                            2 => self.image[0] * 0.5,
                            _ => self.image[1] * 0.5,
                        },
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
    fn exclamation_keeps_stretch_timing_tilt_cycles_and_zero_period_nan() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, n) in [735f32, 159., 1., 60., 3., 5., 7., 60., 80.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = n.to_bits();
        }
        let mut effect = Exclamation::new(&host, [64., 128.]);
        assert_eq!(
            effect.endpoints[0].map(f32::to_bits),
            [0x4441_7c47, 0x42a4_1dfd]
        );
        assert_eq!(
            effect.endpoints[1].map(f32::to_bits),
            [0x443b_a4e9, 0x4300_3932]
        );
        for step in 1u32..=319 {
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
                7 => Some([6, 4, 1, 0, 0x3f25_e354, 0x3f28_f5c3, 178]),
                16 => Some([20, 14, 2, 0, 0x3f3c_b566, 0x3f03_a83b, 255]),
                80 => Some([118, 111, 4, 0x3efa_0674, 0x3f19_999a, 0x3f19_999a, 255]),
                120 => Some([182, 172, 3, 0, 0x3f19_999a, 0x3f19_999a, 255]),
                319 => Some([489, 463, 3, 0, 0x3f19_999a, 0x3f19_999a, 255]),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(
                    [
                        effect.age as u32,
                        effect.phase_time as u32,
                        effect.phase,
                        effect.tilt.to_bits(),
                        effect.current_scale[0].to_bits(),
                        effect.current_scale[1].to_bits(),
                        effect.alpha
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 16 {
                assert_eq!(
                    effect.position.map(f32::to_bits),
                    [0x443b_a4e9, 0x4307_b932]
                );
            }
            if step == 80 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let vertex = mesh.projected_quad(0, &Projection::default()).unwrap()[2];
                assert_eq!(
                    [
                        vertex.position[0].to_bits(),
                        vertex.position[1].to_bits(),
                        vertex.position[2].to_bits(),
                        vertex.rhw.to_bits()
                    ],
                    [0x4434_6dfe, 0x4309_4c7f, 0x3f6c_45b0, 0x3a84_436e]
                );
            }
        }
        host.parameters = [0; 32];
        host.parameters[3] = (-4f32).to_bits();
        let mut effect = Exclamation::new(&host, [64., 128.]);
        assert_eq!(effect.scale, 0.001);
        for step in 1u32..=9 {
            effect.update(
                if step == 9 {
                    7
                } else if step == 7 {
                    0
                } else {
                    1
                },
                [640., 360.],
            );
            if step == 8 {
                assert_eq!(effect.phase, 4);
                assert_eq!(effect.tilt.to_bits(), 0xffc0_0000);
            }
        }
        assert_eq!(effect.phase, 3);
        assert_eq!(effect.tilt.to_bits(), 0);
    }
}
