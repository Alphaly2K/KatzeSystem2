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

pub(super) struct Common {
    age: i32,
    phase_time: i32,
    phase: u32,
    anchor: [f32; 2],
    endpoints: [[f32; 2]; 2],
    center: [f32; 2],
    position: [f32; 2],
    scale: f32,
    current_scale: f32,
    angle: f32,
    lives: [i32; 2],
    image: [f32; 2],
    alpha: u32,
    has_particles: bool,
}

impl Common {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let anchor = [p[0], p[1]];
        let mirror = integer(p[2]) != 0;
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let angle = if mirror { 0.5f32 } else { -0.5 };
        let quantized = integer(angle * 1000.) as f32 / 1000.;
        let cosine = (quantized as f64).cos() as f32;
        let sine = (quantized as f64).sin() as f32;
        // 目标向量保留逐步写回，Y 分量使用已更新的 X。
        let x = ((cosine * 0. - (-sine * 1.)) + 0.) + 0.;
        let y = ((x * sine - cosine * 1.) + 0.) + 0.;
        let direction = [x, y];
        let factor = if image[1] > 256. { 1. } else { 0.5 };
        let endpoints = [300., 100.].map(|distance| {
            std::array::from_fn(|i| (((direction[i] * scale) * distance) * factor) + anchor[i])
        });
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            anchor,
            endpoints,
            center: host.surface_size.map(|n| n as f32 * 0.5),
            position: [anchor[0], anchor[1]],
            scale,
            current_scale: scale,
            angle,
            lives: [integer(p[4]), integer(p[5])],
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
        if self.phase < 2 {
            let from = if self.phase == 0 {
                self.anchor
            } else {
                self.endpoints[0]
            };
            let to = self.endpoints[self.phase as usize];
            let duration = self.lives[self.phase as usize] as f32;
            self.position = std::array::from_fn(|i| fade(from[i], to[i], duration, elapsed as f32));
            self.current_scale = if self.phase == 0 {
                fade(self.scale, self.scale * 1.3, duration, elapsed as f32)
            } else {
                fade(self.scale * 1.3, self.scale, duration, elapsed as f32)
            };
            self.alpha = integer(if self.phase == 0 {
                fade(0., 128., duration, elapsed as f32)
            } else {
                fade(128., 255., duration, elapsed as f32)
            }) as u32
                & 255;
            if elapsed > self.lives[self.phase as usize] {
                self.phase = if self.phase == 0 { 1 } else { 10 };
                self.phase_time = self.age;
            }
        } else {
            self.position = self.endpoints[1];
        }
        for (i, n) in source.into_iter().enumerate() {
            self.position[i] += n - self.center[i];
        }
        self.age = self.age.wrapping_add(integer(delta as f32));
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let mut transform = Transform::default();
        transform.set_scale([self.current_scale, self.current_scale, 1.]);
        transform.set_rotation([0., 0., self.angle]);
        let mut matrix = *transform.matrix();
        matrix[12..14].copy_from_slice(&self.position);
        for quad in &mut host.quads {
            for (i, v) in quad.iter_mut().enumerate() {
                let point = transform_vector(
                    [
                        if i & 1 == 0 {
                            -self.image[0] * 0.5
                        } else {
                            self.image[0] * 0.5
                        },
                        match i {
                            0 | 1 => -self.image[1] * 0.5,
                            // 第三个角点的下沿使用半宽。
                            2 => self.image[0] * 0.5,
                            _ => self.image[1] * 0.5,
                        },
                        0.,
                        1.,
                    ],
                    matrix,
                );
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = 0x00ff_ffff | (self.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn common_keeps_target_rounding_phase_boundaries_and_asymmetric_corners() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [782f32, 169., 1., 60., 5., 8.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Common::new(&host, [64., 128.]);
        assert_eq!(
            effect.endpoints[0].map(f32::to_bits),
            [0x444e_497e, 0x42dd_686e]
        );
        for step in 1u32..=40 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            let source = [
                640. + ((step % 11) as i32 - 5) as f32 * 3.25,
                360. + ((step % 7) as i32 - 3) as f32 * -7.5,
            ];
            effect.update(delta, source);
            let expected = match step {
                7 => [6, 6, 1, 128, 0x444f_e97e, 0x4305_3437, 0x3f47_ae15],
                17 => [21, 15, 10, 255, 0x4447_e87f, 0x4315_9168, 0x3f19_999a],
                40 => [59, 15, 10, 255, 0x4448_b87f, 0x4306_9168, 0x3f19_999a],
                _ => continue,
            };
            assert_eq!(
                [
                    effect.age as u32,
                    effect.phase_time as u32,
                    effect.phase,
                    effect.alpha,
                    effect.position[0].to_bits(),
                    effect.position[1].to_bits(),
                    effect.current_scale.to_bits()
                ],
                expected
            );
            if step == 17 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let vertex = mesh.projected_quad(0, &Projection::default()).unwrap()[2];
                assert_eq!(
                    [vertex.position[0].to_bits(), vertex.position[1].to_bits()],
                    [0x4441_6501, 0x431d_366e]
                );
            }
        }
        let tall = Common::new(&host, [64., 257.]);
        assert!(tall.endpoints[0][0] > effect.endpoints[0][0]);
    }
}
