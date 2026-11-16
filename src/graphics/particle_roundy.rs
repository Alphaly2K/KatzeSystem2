use crate::kcs::{ParticleHost, ScriptRandom, ieee_float_operation};

fn arithmetic(op: u8, a: f32, b: f32) -> f32 {
    f32::from_bits(ieee_float_operation(op, a.to_bits(), b.to_bits()))
}

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fade(from: f32, to: f32, first: f32, last: f32, time: f32) -> f32 {
    if time <= first {
        from
    } else if time >= last {
        to
    } else {
        ((time - first) / (last - first)) * (to - from) + from
    }
}

fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let row = index / 4;
        let col = index % 4;
        let products: [f32; 4] =
            std::array::from_fn(|i| arithmetic(3, left[row * 4 + i], right[col + i * 4]));
        arithmetic(
            1,
            arithmetic(1, arithmetic(1, products[0], products[1]), products[2]),
            products[3],
        )
    })
}

fn matrix(scale: [f32; 3], rotation: [f32; 3], position: [f32; 3]) -> [f32; 16] {
    let mut matrix = [0.; 16];
    for axis in 0..3 {
        matrix[axis * 5] = scale[axis];
    }
    matrix[15] = 1.;
    let identity = std::array::from_fn(|index| if index % 5 == 0 { 1. } else { 0. });
    matrix = multiply(matrix, identity);
    for (axis, angle) in rotation.into_iter().enumerate() {
        let angle = integer(angle * 1000.) as f32 / 1000.;
        let sine = (angle as f64).sin() as f32;
        let cosine = (angle as f64).cos() as f32;
        let mut next = [0.; 16];
        next[15] = 1.;
        let (a, b) = [(1, 2), (2, 0), (0, 1)][axis];
        next[axis * 5] = 1.;
        next[a * 5] = cosine;
        next[b * 5] = cosine;
        next[a * 4 + b] = sine;
        next[b * 4 + a] = -sine;
        matrix = multiply(next, matrix);
    }
    for (axis, value) in position.into_iter().enumerate() {
        matrix[12 + axis] = arithmetic(1, matrix[12 + axis], value);
    }
    matrix
}

fn point(matrix: [f32; 16], value: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        let products: [f32; 3] =
            std::array::from_fn(|i| arithmetic(3, value[i], matrix[axis + i * 4]));
        arithmetic(
            1,
            arithmetic(1, arithmetic(1, products[0], products[1]), products[2]),
            matrix[axis + 12],
        )
    })
}

fn shade(vertices: [[f32; 3]; 4]) -> u8 {
    let a: [f32; 3] = std::array::from_fn(|axis| vertices[1][axis] - vertices[0][axis]);
    let b: [f32; 3] = std::array::from_fn(|axis| vertices[2][axis] - vertices[0][axis]);
    let mut normal = [
        b[2] * a[1] - b[1] * a[2],
        b[0] * a[2] - b[2] * a[0],
        b[1] * a[0] - b[0] * a[1],
    ];
    let square = (normal[1] * normal[1] + normal[0] * normal[0]) + normal[2] * normal[2];
    let length = (square as f64).sqrt() as f32;
    if length > 0. {
        normal = normal.map(|value| value / length);
    }
    integer(fade(
        255.,
        60.,
        -1.,
        1.,
        (normal[1] + normal[0]) - normal[2],
    )) as u8
}

struct Head {
    position: [f32; 3],
    scale: f32,
    angle: f32,
    velocity: f32,
    age: f32,
    phase_time: f32,
    phase: u32,
    alpha: i32,
    lives: [i32; 3],
    vertices: [[[f32; 3]; 4]; 4],
    colors: [u32; 4],
}

impl Head {
    fn spawn(random: &mut ScriptRandom) -> Self {
        let angle = (random.next_float() as f64 * std::f32::consts::PI as f64 * 2.) as f32;
        let distance = (random.next_float() as f64 * 100.) as f32;
        let ratio = distance / 100.;
        let scale = ratio * 0.3 + 0.7;
        let velocity = (random.next_float() * 0.1 + 0.01) * (ratio * 0.5 + 0.5);
        let lives = [10, 5, 40].map(|duration| {
            let duration = duration as f32;
            integer((random.next_float() - 0.5) * duration + duration)
        });
        Self {
            position: [ratio * 100., -distance, 0.],
            scale,
            angle,
            velocity,
            age: 0.,
            phase_time: 0.,
            phase: 0,
            alpha: 0,
            lives,
            vertices: [[[0.; 3]; 4]; 4],
            colors: [u32::MAX; 4],
        }
    }
}

pub(super) struct Roundy {
    anchor: [f32; 3],
    scale: f32,
    heads: Vec<Head>,
}

impl Roundy {
    pub(super) fn new(host: &ParticleHost, random: &mut ScriptRandom) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let scale = p[2] / 100.;
        Self {
            anchor: [p[0], p[1], 0.],
            scale: if scale < 0.001 { 0.001 } else { scale },
            heads: (0..host.quads.len() / 5)
                .map(|_| Head::spawn(random))
                .collect(),
        }
    }

    pub(super) fn update(&mut self, delta: u32, random: &mut ScriptRandom) {
        let delta = integer(delta as f32) as f32;
        for head in &mut self.heads {
            let elapsed = head.age - head.phase_time;
            match head.phase {
                0..=2 => {
                    let phase = head.phase as usize;
                    let factor = if phase == 2 {
                        [1.005, 1.001]
                    } else {
                        [1.001, 1.005]
                    };
                    for (position, factor) in head.position.iter_mut().zip(factor) {
                        *position *= factor;
                    }
                    if phase != 1 {
                        let (from, to) = if phase == 0 { (0., 100.) } else { (100., 0.) };
                        head.alpha = integer(fade(from, to, 0., head.lives[phase] as f32, elapsed));
                    }
                    if phase == 2 {
                        head.velocity *= 0.98;
                    }
                    head.angle += head.velocity;
                    if elapsed > head.lives[phase] as f32 {
                        head.phase = if phase == 2 { 9 } else { head.phase + 1 };
                        head.phase_time = head.age;
                    }
                }
                _ => *head = Head::spawn(random),
            }
            let global = matrix(
                [self.scale, self.scale, 1.],
                [0., head.angle, 0.],
                self.anchor,
            );
            let local = matrix(
                [head.scale; 3],
                [-0.1, std::f32::consts::PI, 1.070_796_4],
                head.position,
            );
            let matrix = multiply(local, global);
            let rows = [(45., -51.), (50., -33.), (52., 0.), (50., 33.), (45., 51.)];
            for face in 0..4 {
                head.vertices[face] = std::array::from_fn(|vertex| {
                    let (y, z) = rows[face + (vertex >> 1)];
                    point(matrix, [if vertex & 1 == 0 { -100. } else { 100. }, y, z])
                });
                let color = u32::from(shade(head.vertices[face]));
                head.colors[face] = (color * 0x010101) | ((head.alpha as u32 & 255) << 24);
            }
            head.age += delta;
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = 0;
        let rows = [0.05, 0.25, 0.5, 0.75, 0.95];
        for (index, head) in self.heads.iter().enumerate() {
            for vertex in &mut host.quads[index] {
                vertex.color &= 0xffffff;
            }
            for face in 0..4 {
                let quad = &mut host.quads[self.heads.len() + index * 4 + face];
                for (vertex, output) in quad.iter_mut().enumerate() {
                    output.position = head.vertices[face][vertex];
                    output.color = head.colors[face];
                    output.uv = [
                        if vertex & 1 == 0 { 0.2 } else { 0.8 },
                        rows[face + (vertex >> 1)],
                    ];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_faces_keep_frame_motion_strict_phases_and_special_matrix_values() {
        let mut host = ParticleHost::default();
        host.reset_quads(6, [64., 128.]).unwrap();
        host.parameters[..3].copy_from_slice(&[640f32, 1010., 1200.].map(f32::to_bits));
        let mut random = ScriptRandom::seeded(17);
        let mut effect = Roundy::new(&host, &mut random);
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..6 {
            expected.next_float();
        }
        assert_eq!(effect.heads[0].lives, [5, 2, 32]);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[0][0].color >> 24, 0);
        assert_eq!(host.quads[1][0].position, [0.; 3]);
        assert_eq!(host.quads[5][0].color, u32::MAX);
        effect.update(0, &mut random);
        effect.write_mesh(&mut host);
        assert_eq!(
            host.quads[1][0].position.map(f32::to_bits),
            [0xc3de_d9c0, 0x438e_f142, 0x410f_42c1]
        );
        assert_eq!(host.quads[1][0].color, 0x003c3c3c);
        effect.update(0, &mut random);
        effect.write_mesh(&mut host);
        assert_eq!(
            host.quads[1][0].position.map(f32::to_bits),
            [0xc3dc_6a94, 0x438e_7d1e, 0x4147_bc50]
        );
        for delta in [5, 0, 1] {
            effect.update(delta, &mut random);
            assert_eq!(effect.heads[0].phase, 0);
        }
        effect.update(0, &mut random);
        assert_eq!(effect.heads[0].phase, 1);
        for (delta, phase) in [(3, 1), (0, 2), (33, 2), (0, 9)] {
            effect.update(delta, &mut random);
            assert_eq!(effect.heads[0].phase, phase);
        }
        assert_eq!(random.clone().next_float(), expected.clone().next_float());
        effect.update(0, &mut random);
        for _ in 0..6 {
            expected.next_float();
        }
        assert_eq!(random.next_integer(), expected.next_integer());
        assert_eq!(random.next_float(), expected.next_float());
        assert_eq!(effect.heads[0].phase, 0);
        for (scale, nan) in [(f32::INFINITY, 0xffc0_0000), (f32::NAN, 0x7fc0_0000)] {
            host.parameters[2] = scale.to_bits();
            let mut effect = Roundy::new(&host, &mut random);
            effect.update(0, &mut random);
            effect.write_mesh(&mut host);
            assert!(host.quads[1..5].iter().flatten().all(|vertex| {
                vertex.position.map(f32::to_bits) == [nan; 3] && vertex.color == 0
            }));
        }
    }
}
