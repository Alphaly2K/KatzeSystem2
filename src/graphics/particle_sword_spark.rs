use crate::kcs::{ParticleHost, ScriptRandom};
use crate::{Error, Result};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn integer(value: f64) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

// 每个矩阵输出使用较宽精度累加，在写回时才舍入到 f32。
fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let (row, column) = (index / 4, index % 4);
        let term = |i| left[row * 4 + i] as f64 * right[i * 4 + column] as f64;
        (((term(1) + term(0)) + term(2)) + term(3)) as f32
    })
}

fn point(position: [f32; 3], matrix: [f32; 16]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        (((position[1] as f64 * matrix[4 + axis] as f64
            + position[0] as f64 * matrix[axis] as f64)
            + position[2] as f64 * matrix[8 + axis] as f64)
            + matrix[12 + axis] as f64) as f32
    })
}

fn rotation(axis: usize, angle: f32) -> [f32; 16] {
    // 旋转辅助函数先将弧度向零量化到千分之一。
    let angle = (integer(angle as f64 * 1000.) as f64 / 1000.) as f32 as f64;
    let (s, c) = (angle.sin() as f32, angle.cos() as f32);
    let mut matrix = IDENTITY;
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    matrix[a * 4 + a] = c;
    matrix[b * 4 + b] = c;
    matrix[a * 4 + b] = s;
    matrix[b * 4 + a] = -s;
    matrix
}

fn linear(from: f32, to: f32, start: f32, end: f32, time: f32) -> f32 {
    if time <= start {
        from
    } else if time >= end {
        to
    } else {
        ((time - start) / (end - start)) * (to - from) + from
    }
}

struct Particle {
    age: i32,
    phase_value: i32,
    phase: u32,
    velocity: [f32; 3],
    position: [f32; 3],
    rotation: [f32; 3],
    scale: f32,
    rotation_step: [f32; 3],
    matrix: [f32; 16],
    base: [[f32; 3]; 4],
    vertices: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    color: u32,
}

pub(super) struct SwordSpark {
    mode: u32,
    particles: Vec<Particle>,
}

impl SwordSpark {
    pub(super) fn new(
        host: &ParticleHost,
        image: [f32; 2],
        random: &mut ScriptRandom,
    ) -> Result<Self> {
        let p = host.parameters.map(f32::from_bits);
        let regions = integer(p[5] as f64) as u32;
        if regions > 4 && !host.quads.is_empty() {
            return Err(Error::new(0, "剑光火花图集分区数量超过上限"));
        }
        let mut particles = Vec::with_capacity(host.quads.len());
        for index in 0..host.quads.len() {
            let region = if regions != 0 {
                random.next_integer() % regions
            } else {
                0
            };
            let start = if regions == 0 {
                [0.; 2]
            } else {
                [(region % 2) as f32 * 0.5, (region / 2) as f32 * 0.5]
            };
            let extent = if regions == 0 { 1. } else { 0.5 };
            let uv = std::array::from_fn(|i| {
                [
                    start[0] + (i & 1) as f32 * extent,
                    start[1] + (i >> 1) as f32 * extent,
                ]
            });
            let scale = (p[2] as f64 / 1000. * (random.next_float() as f64 + 1.)) as f32;
            let degrees = ((360. / host.quads.len() as f64) as f32 as f64 * index as f64) as f32;
            let angle = (degrees as f64 * std::f32::consts::PI as f64 / 180.) as f32;
            let angle = (integer(angle as f64 * 100.) as f64 / 100.) as f32 as f64;
            let (s, c) = (angle.sin() as f32, angle.cos() as f32);
            let mut velocity = [
                (c as f64 * 0. - s as f64) as f32,
                (s as f64 * 0. + c as f64) as f32,
                0.5,
            ];
            let squares = velocity.map(|n| n * n);
            let length = (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32;
            if length > 0. {
                velocity = velocity.map(|n| n / length);
            }
            let speed = (random.next_float() as f64 * p[3] as f64 + p[3] as f64 * 0.5) as f32;
            for value in &mut velocity[..2] {
                *value = (*value as f64 * speed as f64) as f32;
            }
            velocity[2] = (speed as f64 * 3. * velocity[2] as f64) as f32;
            let mut angles = std::array::from_fn(|_| {
                (random.next_float() as f64 * std::f32::consts::PI as f64) as f32
            });
            let rotation_step = std::array::from_fn(|_| {
                (random.next_float() as f64 * 0.3f32 as f64 + 0.3f32 as f64) as f32
            });
            for angle in &mut angles {
                if random.next_integer() & 1 != 0 {
                    *angle = -*angle;
                }
            }
            let base = std::array::from_fn(|i| {
                [
                    if i & 1 == 0 {
                        -image[0] * 0.5
                    } else {
                        image[0] * 0.5
                    },
                    if i < 2 {
                        -image[1] * 0.5
                    } else {
                        image[1] * 0.5
                    },
                    0.,
                ]
            });
            let mut matrix = IDENTITY;
            matrix[12..14].copy_from_slice(&p[..2]);
            particles.push(Particle {
                age: 0,
                phase_value: 0,
                phase: 0,
                velocity,
                position: [p[0], p[1], 0.],
                rotation: angles,
                scale,
                rotation_step,
                matrix,
                base,
                vertices: base.map(|v| point(v, matrix)),
                uv,
                color: 0x00ffffff,
            });
        }
        Ok(Self {
            mode: if p[4] < 1. { 1 } else { host.render_mode },
            particles,
        })
    }

    pub(super) fn update(&mut self, delta: u32, parameters: &[u32; 32]) {
        let p = parameters.map(f32::from_bits);
        let increment = integer(delta as f32 as f64);
        for particle in &mut self.particles {
            if particle.phase < 3 {
                let (movement, spin) = if particle.phase == 0 {
                    (1., 1.)
                } else {
                    (0.3f32 as f64, 0.5)
                };
                for axis in 0..3 {
                    particle.position[axis] = (particle.position[axis] as f64
                        + particle.velocity[axis] as f64 * movement)
                        as f32;
                    particle.rotation[axis] = (particle.rotation[axis] as f64
                        + particle.rotation_step[axis] as f64 * spin)
                        as f32;
                }
                let mut alpha = if particle.phase == 2 {
                    integer(linear(
                        particle.phase_value as f32,
                        0.,
                        p[7],
                        p[8],
                        particle.age as f32,
                    ) as f64) as u32
                } else {
                    integer(p[4] as f64 * 64. + 191.) as u32
                };
                let end = p[6 + particle.phase as usize];
                if particle.age as f64 > end as f64 {
                    particle.phase += 1;
                    particle.phase_value = if particle.phase == 3 {
                        particle.age
                    } else {
                        alpha as u8 as i32
                    };
                    if particle.phase == 3 {
                        alpha = 0;
                    }
                }
                particle.color = (particle.color & 0xffffff) | ((alpha & 255) << 24);
            }
            particle.matrix = IDENTITY;
            for axis in 0..3 {
                particle.matrix =
                    multiply(rotation(axis, particle.rotation[axis]), particle.matrix);
            }
            particle.matrix[12..15].copy_from_slice(&particle.position);
            let mut scale = IDENTITY;
            scale[0] = particle.scale;
            scale[5] = particle.scale;
            particle.matrix = multiply(scale, particle.matrix);
            particle.vertices = particle.base.map(|v| point(v, particle.matrix));
            let a: [f32; 3] =
                std::array::from_fn(|i| particle.vertices[1][i] - particle.vertices[0][i]);
            let b: [f32; 3] =
                std::array::from_fn(|i| particle.vertices[2][i] - particle.vertices[0][i]);
            let mut normal = std::array::from_fn::<_, 3, _>(|i| {
                let (j, k) = ((i + 1) % 3, (i + 2) % 3);
                (a[j] as f64 * b[k] as f64 - a[k] as f64 * b[j] as f64) as f32
            });
            let length = (((normal[0] as f64 * normal[0] as f64
                + normal[1] as f64 * normal[1] as f64)
                + normal[2] as f64 * normal[2] as f64) as f32 as f64)
                .sqrt() as f32;
            if length > 0. {
                normal = normal.map(|n| n / length);
            }
            let light = (normal[0] as f64 + normal[1] as f64 - normal[2] as f64) as f32;
            let shade = integer(linear(255., 128., -1., 1., light) as f64) as u8 as u32;
            particle.color = (particle.color & 0xff000000) | (shade * 0x010101);
            particle.age = particle.age.wrapping_add(increment);
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        for (quad, particle) in host.quads.iter_mut().zip(&self.particles) {
            for (index, vertex) in quad.iter_mut().enumerate() {
                vertex.position = particle.vertices[index];
                vertex.uv = particle.uv[index];
                vertex.color = particle.color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_uses_update_phases_while_age_and_fade_use_the_clock() -> Result<()> {
        let mut host = ParticleHost::default();
        host.reset_quads(1, [12., 20.])?;
        for (index, value) in [9f32, -17., 640., 9., 0.25, 4., 0., 0., 0.]
            .into_iter()
            .enumerate()
        {
            host.parameters[index] = value.to_bits();
        }
        host.render_mode = 3;
        let mut random = ScriptRandom::seeded(17);
        let mut expected = random.clone();
        for _ in 0..4 {
            expected.next_integer();
        }
        for _ in 0..8 {
            expected.next_float();
        }
        let mut effect = SwordSpark::new(&host, [12., 20.], &mut random)?;
        effect.write_mesh(&mut host);
        assert_eq!(host.render_mode, 1);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        let start = host.quads[0][0].position;
        effect.update(0, &host.parameters);
        effect.write_mesh(&mut host);
        assert_eq!((effect.particles[0].age, effect.particles[0].phase), (0, 0));
        assert_ne!(host.quads[0][0].position, start);
        assert_eq!(host.quads[0][0].color >> 24, 207);
        effect.update(1, &host.parameters);
        assert_eq!(effect.particles[0].phase, 0);
        effect.update(0, &host.parameters);
        assert_eq!(
            (effect.particles[0].phase, effect.particles[0].phase_value),
            (1, 207)
        );
        effect.update(0, &host.parameters);
        assert_eq!(effect.particles[0].phase, 2);
        effect.update(0, &host.parameters);
        effect.write_mesh(&mut host);
        assert_eq!(effect.particles[0].phase, 3);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        let terminal = host.quads.clone();
        effect.update(90, &host.parameters);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads, terminal);
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(
            random.next_float().to_bits(),
            expected.next_float().to_bits()
        );
        host.parameters[5] = 5f32.to_bits();
        assert!(SwordSpark::new(&host, [12., 20.], &mut random).is_err());
        host.reset_quads(0, [12., 20.])?;
        let empty = SwordSpark::new(&host, [12., 20.], &mut random)?;
        assert!(empty.particles.is_empty());
        Ok(())
    }
}
