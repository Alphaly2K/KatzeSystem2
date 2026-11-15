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

pub(super) struct Surprise {
    age: i32,
    phase_time: i32,
    phase: u32,
    anchor: [f32; 2],
    mirror: i32,
    scale: f32,
    lives: [i32; 2],
    center: [f32; 2],
    factor: f32,
    image: [f32; 2],
    position: [f32; 2],
    current_scale: f32,
    angle: f32,
    alpha: u32,
    has_particles: bool,
}

impl Surprise {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            anchor: [p[0], p[1]],
            mirror: integer(p[2]),
            scale,
            lives: [integer(p[4]), integer(p[5])],
            center: host.surface_size.map(|n| n as f32 * 0.5),
            factor: if image[1] > 256. { 1. } else { 0.5 },
            image,
            position: [p[0], p[1]],
            current_scale: scale * 3.,
            angle: 0.,
            alpha: 0,
            has_particles: !host.quads.is_empty(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if !self.has_particles {
            return;
        }
        let elapsed = self.age.wrapping_sub(self.phase_time);
        self.position = self.anchor;
        match self.phase {
            0 => {
                self.angle = fade(
                    0.,
                    std::f32::consts::TAU * 2.,
                    self.lives[0] as f32,
                    elapsed as f32,
                );
                if self.mirror == 1 {
                    self.angle *= -1.;
                }
                self.current_scale = fade(
                    self.scale * 3.,
                    self.scale,
                    self.lives[0] as f32,
                    elapsed as f32,
                );
                self.alpha =
                    integer(fade(0., 255., self.lives[0] as f32, elapsed as f32)) as u32 & 255;
                if elapsed > self.lives[0] {
                    self.phase = 1;
                    self.phase_time = self.age;
                    self.angle = 0.;
                }
            }
            1 => {
                let amount = fade(
                    (self.scale * 35.) * self.factor,
                    0.,
                    self.lives[1] as f32,
                    elapsed as f32,
                );
                let signs = match elapsed % 4 {
                    0 => [-1., -1.],
                    1 => [1., 1.],
                    2 => [1., -1.],
                    3 => [-1., 1.],
                    _ => [0.; 2],
                };
                for (i, sign) in signs.into_iter().enumerate() {
                    self.position[i] += sign * amount;
                }
                if elapsed > self.lives[1] {
                    self.phase = 10;
                    self.phase_time = self.age;
                }
            }
            _ => {}
        }
        for (i, value) in source.into_iter().enumerate() {
            self.position[i] += value - self.center[i];
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
            for (i, vertex) in quad.iter_mut().enumerate() {
                let x = if (i & 1 == 0) == (self.mirror == 0) {
                    -self.image[0] * 0.5
                } else {
                    self.image[0] * 0.5
                };
                let y = match i {
                    0 | 1 => -self.image[1] * 0.5,
                    // 第三个角点的下沿沿用半宽。
                    2 => self.image[0] * 0.5,
                    _ => self.image[1] * 0.5,
                };
                let point = transform_vector([x, y, 0., 1.], matrix);
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
    fn surprise_keeps_rotation_jitter_boundaries_and_mirrored_corners() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [804f32, 140., 1., 78., 10., 35.].into_iter().enumerate() {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Surprise::new(&host, [64., 128.]);
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
                7 => Some([
                    6,
                    0,
                    0,
                    0x444a_a000,
                    0x4322_8000,
                    0xc0f1_463b,
                    0x3fb3_b645,
                    153,
                ]),
                11 => Some([16, 14, 1, 0x4448_40a4, 0x4311_c28f, 0, 0x3f47_ae14, 255]),
                13 => Some([18, 14, 1, 0x4443_7148, 0x4301_fae1, 0, 0x3f47_ae14, 255]),
                40 => Some([59, 55, 10, 0x444a_a000, 0x42fa_0000, 0, 0x3f47_ae14, 255]),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(
                    [
                        effect.age as u32,
                        effect.phase_time as u32,
                        effect.phase,
                        effect.position[0].to_bits(),
                        effect.position[1].to_bits(),
                        effect.angle.to_bits(),
                        effect.current_scale.to_bits(),
                        effect.alpha
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 7 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                let quad = mesh.projected_quad(0, &Projection::default()).unwrap();
                assert_eq!(
                    [
                        quad[0].position[0].to_bits(),
                        quad[0].position[1].to_bits(),
                        quad[2].position[0].to_bits(),
                        quad[2].position[1].to_bits()
                    ],
                    [0x4438_bb39, 0x42b8_0208, 0x4458_c734, 0x4305_a788]
                );
                assert_eq!((quad[0].color, quad[2].uv), (0x99ff_ffff, [0., 1.]));
            }
        }
        host.parameters[2] = (-1f32).to_bits();
        host.parameters[3] = 0;
        let mut other_mirror = Surprise::new(&host, [64., 128.]);
        assert_eq!(other_mirror.scale, 0.001);
        other_mirror.update(1, [640., 360.]);
        other_mirror.write_mesh(&mut host);
        assert!(host.quads[0][0].position[0] > host.quads[0][1].position[0]);
        other_mirror.update(1, [640., 360.]);
        assert!(other_mirror.angle > 0.);
    }
}
