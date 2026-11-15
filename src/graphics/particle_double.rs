use super::Transform;
use crate::kcs::{ParticleHost, transform_vector};

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn linear(from: f32, to: f32, end: f32, time: f32) -> f32 {
    if time <= 0. {
        from
    } else if time >= end {
        to
    } else {
        (time / end) * (to - from) + from
    }
}

struct Particle {
    age: i32,
    phase_time: i32,
    phase: u32,
    position: [f32; 3],
    scale: f32,
    alpha: u32,
}

pub(super) struct Double {
    anchor: [f32; 2],
    angle: f32,
    scale: f32,
    lives: [i32; 3],
    factor: f32,
    center: [f32; 2],
    image: [f32; 2],
    particles: Vec<Particle>,
}

impl Double {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        let anchor = [p[0], p[1]];
        Self {
            anchor,
            angle: if integer(p[2]) == 0 { -0.4 } else { 0.4 },
            scale,
            lives: std::array::from_fn(|i| integer(p[i + 4])),
            factor: if image[1] > 256. { 1. } else { 0.5 },
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            particles: (0..host.quads.len())
                .map(|_| Particle {
                    age: 0,
                    phase_time: 0,
                    phase: 0,
                    position: [anchor[0], anchor[1], 0.],
                    scale: scale * 3.,
                    alpha: 0,
                })
                .collect(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        let elapsed = integer(delta as f32);
        for p in &mut self.particles {
            let time = p.age.wrapping_sub(p.phase_time);
            match p.phase {
                0 => {
                    p.position = [self.anchor[0], self.anchor[1], 0.];
                    p.scale = linear(
                        self.scale * 3.,
                        self.scale,
                        self.lives[0] as f32,
                        time as f32,
                    );
                    p.alpha =
                        integer(linear(0., 255., self.lives[0] as f32, time as f32)) as u32 & 255;
                    if time > self.lives[0] {
                        p.phase = 1;
                        p.phase_time = p.age;
                    }
                }
                1 => {
                    let distance = linear(
                        (self.scale * 35.) * self.factor,
                        0.,
                        self.lives[1] as f32,
                        time as f32,
                    );
                    let position = match time % 4 {
                        0 => [self.anchor[0] - distance, self.anchor[1] - distance],
                        1 => [self.anchor[0] + distance, self.anchor[1] + distance],
                        2 => [self.anchor[0] + distance, self.anchor[1] - distance],
                        3 => [self.anchor[0] - distance, self.anchor[1] + distance],
                        _ => [p.position[0], p.position[1]],
                    };
                    p.position[..2].copy_from_slice(&position);
                    if time > self.lives[1] {
                        p.phase = 2;
                        p.phase_time = p.age;
                    }
                }
                2 => {
                    p.position = [self.anchor[0], self.anchor[1], 0.];
                    if time > self.lives[2] {
                        p.phase = 1;
                        p.phase_time = p.age;
                    }
                }
                _ => {
                    *p = Particle {
                        age: 0,
                        phase_time: 0,
                        phase: 0,
                        position: [self.anchor[0], self.anchor[1], 0.],
                        scale: self.scale * 3.,
                        alpha: 0,
                    };
                }
            }
            for (i, value) in offset.into_iter().enumerate() {
                p.position[i] += value;
            }
            p.age = p.age.wrapping_add(elapsed);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [width, height] = self.image.map(|n| n * 0.5);
        for (p, quad) in self.particles.iter().zip(&mut host.quads) {
            let mut transform = Transform::default();
            transform.set_scale([p.scale, p.scale, 1.]);
            transform.set_rotation([0., 0., self.angle]);
            let mut matrix = *transform.matrix();
            matrix[12..15].copy_from_slice(&p.position);
            for (i, v) in quad.iter_mut().enumerate() {
                // 第三个顶点的纵坐标取半宽，非方形纹理也保留这一布局。
                let position = [
                    if i & 1 == 0 { -width } else { width },
                    if i < 2 {
                        -height
                    } else if i == 2 {
                        width
                    } else {
                        height
                    },
                    0.,
                    1.,
                ];
                let point = transform_vector(position, matrix);
                v.position.copy_from_slice(&point[..3]);
                v.rhw = point[3];
                v.color = 0xffffff | (p.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(image: [f32; 2], parameters: &[f32]) -> ParticleHost {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, image).unwrap();
        for (i, n) in parameters.iter().enumerate() {
            host.parameters[i] = n.to_bits();
        }
        host
    }

    #[test]
    fn shake_and_hold_repeat_after_strict_boundaries_and_keep_source_offsets() {
        let host = host([64., 128.], &[100., 200., 0., 100., 2., 2., 1.]);
        let mut effect = Double::new(&host, [64., 128.]);
        effect.update(2, [640., 360.]);
        assert_eq!(
            (effect.particles[0].scale, effect.particles[0].alpha),
            (3., 0)
        );
        effect.update(1, [640., 360.]);
        let p = &effect.particles[0];
        assert_eq!((p.age, p.phase, p.scale, p.alpha), (3, 0, 1., 255));
        effect.update(0, [640., 360.]);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (1, 3)
        );
        effect.update(1, [650., 353.]);
        assert_eq!(effect.particles[0].position, [92.5, 175.5, 0.]);
        effect.update(1, [650., 353.]);
        assert_eq!(effect.particles[0].position, [118.75, 201.75, 0.]);
        effect.update(2, [650., 353.]);
        assert_eq!(effect.particles[0].position, [110., 193., 0.]);
        assert_eq!(effect.particles[0].phase, 1);
        effect.update(0, [640., 360.]);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (2, 7)
        );
        effect.update(1, [640., 360.]);
        effect.update(1, [640., 360.]);
        assert_eq!(effect.particles[0].phase, 2);
        effect.update(0, [640., 360.]);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_time),
            (1, 9)
        );
        effect.update(0, [640., 360.]);
        let p = &effect.particles[0];
        assert_eq!((p.position, p.scale, p.alpha), ([82.5, 182.5, 0.], 1., 255));
    }

    #[test]
    fn non_square_geometry_preserves_the_third_corner_facing_and_minimum_scale() {
        use crate::graphics::{ParticleMesh, Projection};

        for (image, parameters, expected) in [
            (
                [64., 128.],
                [810f32, 181., 1., 60., 5., 5., 5., 5., 5.],
                0xce72_4fb3_835f_4e36u64,
            ),
            (
                [96., 256.],
                [-35.25, 99.5, 0., -4., 0., -1., 2., 8., 1.],
                0x5471_df24_35a6_0687,
            ),
            (
                [96., 300.],
                [12.5, -8., 2., 37.125, 1., 2., 0., 1., 3.],
                0x571a_62c6_0985_8488,
            ),
            (
                [64., 128.],
                [0., 0., 0.5, 0., 1., 0., 0., 0., 0.],
                0xc7ca_0680_3402_eed8,
            ),
        ] {
            let mut host = host(image, &parameters);
            Double::new(&host, image).write_mesh(&mut host);
            host.depth_offset = 5000.;
            let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
            let quad = mesh.projected_quad(0, &Projection::default()).unwrap();
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for v in quad {
                for word in [
                    v.position[0].to_bits(),
                    v.position[1].to_bits(),
                    v.position[2].to_bits(),
                    v.rhw.to_bits(),
                    v.color,
                    v.uv[0].to_bits(),
                    v.uv[1].to_bits(),
                ] {
                    for byte in word.to_le_bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
            assert_eq!(hash, expected, "image={image:?}, parameters={parameters:?}");
        }
    }
}
