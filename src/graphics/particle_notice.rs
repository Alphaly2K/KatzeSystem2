use crate::kcs::{ParticleHost, transform_vector};

pub(super) struct Notice {
    age: i32,
    phase_time: i32,
    phase: usize,
    alpha: u32,
    durations: [i32; 5],
    anchor: [f32; 2],
    center: [f32; 2],
    position: [f32; 2],
    image: [f32; 2],
    scale: f32,
    mirrored: bool,
    has_particles: bool,
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

impl Notice {
    pub(super) fn new(particles: &ParticleHost, image: [f32; 2]) -> Self {
        let parameters = particles.parameters.map(f32::from_bits);
        let mut scale = parameters[3] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        Self {
            age: 0,
            phase_time: 0,
            phase: 0,
            alpha: 0,
            durations: std::array::from_fn(|i| integer(parameters[i + 4])),
            anchor: [parameters[0], parameters[1]],
            center: particles.surface_size.map(|n| n as f32 * 0.5),
            position: [parameters[0], parameters[1]],
            image,
            scale,
            mirrored: integer(parameters[2]) != 0,
            has_particles: !particles.quads.is_empty(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        self.position = std::array::from_fn(|i| self.anchor[i] + (source[i] - self.center[i]));
        if !self.has_particles {
            return;
        }
        if self.phase < self.durations.len() {
            self.alpha = if self.phase.is_multiple_of(2) { 255 } else { 0 };
            if self.age.wrapping_sub(self.phase_time) > self.durations[self.phase] {
                self.phase += 1;
                self.phase_time = self.age;
            }
        }
        self.age = self.age.wrapping_add(integer(delta as f32));
    }

    pub(super) fn write_mesh(&self, particles: &mut ParticleHost) {
        let mut matrix = [0.; 16];
        matrix[0] = self.scale;
        matrix[5] = self.scale;
        matrix[10] = 1.;
        matrix[15] = 1.;
        matrix[12..14].copy_from_slice(&self.position);
        let half = self.image.map(|n| n * 0.5);
        for quad in &mut particles.quads {
            for (i, vertex) in quad.iter_mut().enumerate() {
                let left = (i & 1 == 0) != self.mirrored;
                let point = transform_vector(
                    [
                        if left { -half[0] } else { half[0] },
                        // 基线第三个顶点的 Y 使用半宽，形成不对称下沿。
                        match i {
                            0 | 1 => -half[1],
                            2 => half[0],
                            _ => half[1],
                        },
                        0.,
                        1.,
                    ],
                    matrix,
                );
                vertex.position.copy_from_slice(&point[..3]);
                vertex.rhw = point[3];
                vertex.color = 0x00ff_ffff | (self.alpha << 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_keeps_strict_blink_boundaries_and_asymmetric_mirrored_corners() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(1, [64., 128.]).unwrap();
        for (i, value) in [810f32, 181., 1., 60., 5., 5., 5., 5., 5.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Notice::new(&host, [64., 128.]);
        for step in 1u32..=40 {
            let delta = if step.is_multiple_of(9) {
                7
            } else if step.is_multiple_of(7) {
                0
            } else {
                1
            };
            effect.update(delta, [640., 360.]);
            match step {
                6 => assert_eq!(
                    (effect.age, effect.phase_time, effect.phase, effect.alpha),
                    (6, 0, 0, 255)
                ),
                7 => assert_eq!(
                    (effect.age, effect.phase_time, effect.phase, effect.alpha),
                    (6, 6, 1, 255)
                ),
                8 => assert_eq!(effect.alpha, 0),
                10 => assert_eq!(
                    (effect.age, effect.phase_time, effect.phase, effect.alpha),
                    (15, 14, 2, 0)
                ),
                40 => assert_eq!(
                    (effect.age, effect.phase_time, effect.phase, effect.alpha),
                    (59, 34, 5, 255)
                ),
                _ => {}
            }
        }
        effect.write_mesh(&mut host);
        let quad = host.quads[0];
        assert!(quad[0].position[0] > quad[1].position[0]);
        assert_eq!(quad[2].position[1], 200.2);
        assert_eq!(quad[3].position[1], 219.4);
        assert_eq!(quad[2].uv, [0., 1.]);
        assert!(quad.iter().all(|v| v.color == u32::MAX && v.rhw == 1.));
    }
}
