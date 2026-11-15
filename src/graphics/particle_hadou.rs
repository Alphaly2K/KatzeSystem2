use crate::kcs::{ParticleHost, transform_vector};
use crate::{Error, Result};

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

struct Trail {
    age: i32,
    phase: u32,
    position: [f32; 3],
    scale: [f32; 2],
    alpha: u32,
}

struct Head {
    age: i32,
    phase_time: i32,
    phase: u32,
    position: [f32; 3],
    scale: [f32; 2],
    alpha: u32,
    trails: Vec<Trail>,
}

impl Head {
    fn restart(&mut self, anchor: [f32; 2], scale: f32) {
        self.age = 0;
        self.phase_time = 0;
        self.phase = 0;
        self.position = [anchor[0], anchor[1], 0.];
        self.scale = [scale; 2];
        self.alpha = 0;
        for (index, trail) in self.trails.iter_mut().enumerate() {
            trail.age = 0;
            trail.phase = 0;
            trail.position = [anchor[0], anchor[1], (index + 1) as f32];
            trail.alpha = 0;
        }
    }
}

pub(super) struct Hadou {
    anchor: [f32; 2],
    scale: [f32; 2],
    interval: u32,
    opacity: i32,
    lives: [i32; 3],
    center: [f32; 2],
    image: [f32; 2],
    visible: u32,
    updates: u32,
    heads: Vec<Head>,
}

impl Hadou {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Result<Self> {
        let p = host.parameters.map(f32::from_bits);
        let interval = integer(p[5]);
        if interval == 0 {
            return Err(Error::new(0, "波动粒子的激活间隔不能为零"));
        }
        let group = integer(p[0]) as u32 as usize;
        let count = host.quads.len().checked_div(group).unwrap_or(host.quads.len());
        let scale = [p[3], p[4]].map(|n| {
            let n = n / 100.;
            if n < 0.001 { 0.001 } else { n }
        });
        let mut effect = Self {
            anchor: [p[1], p[2]],
            scale,
            interval: interval.max(1) as u32,
            opacity: integer(p[6]),
            lives: std::array::from_fn(|i| integer(p[i + 7])),
            center: host.surface_size.map(|n| n as f32 * 0.5),
            image,
            visible: 0,
            updates: 0,
            heads: Vec::with_capacity(count),
        };
        for _ in 0..count {
            let mut head = Head {
                age: 0,
                phase_time: 0,
                phase: 0,
                position: [0.; 3],
                scale: [1.; 2],
                alpha: 0,
                trails: (0..group.saturating_sub(1))
                    .map(|_| Trail {
                        age: 0,
                        phase: 0,
                        position: [0.; 3],
                        scale: [1.; 2],
                        alpha: 0,
                    })
                    .collect(),
            };
            head.restart(effect.anchor, effect.scale[0]);
            effect.heads.push(head);
        }
        Ok(effect)
    }

    pub(super) fn update(&mut self, delta: u32, source: [f32; 2]) {
        if self.updates.is_multiple_of(self.interval) {
            self.visible = self.visible.wrapping_add(1);
        }
        self.updates = self.updates.wrapping_add(1);
        self.visible = self.visible.min(self.heads.len() as u32);
        let offset: [f32; 2] = std::array::from_fn(|i| source[i] - self.center[i]);
        for head in &mut self.heads[..self.visible as usize] {
            let elapsed = head.age.wrapping_sub(head.phase_time);
            match head.phase {
                0 | 1 => {
                    head.position[..2].copy_from_slice(&self.anchor);
                    let scale = fade(
                        self.scale[0],
                        self.scale[1],
                        self.lives[0].wrapping_add(self.lives[1]) as f32,
                        head.age as f32,
                    );
                    head.scale = [scale * 1.1, scale];
                    let (from, to) = if head.phase == 0 {
                        (0., self.opacity as f32)
                    } else {
                        (self.opacity as f32, 0.)
                    };
                    head.alpha = integer(fade(
                        from,
                        to,
                        self.lives[head.phase as usize] as f32,
                        elapsed as f32,
                    )) as u32
                        & 255;
                    if elapsed > self.lives[head.phase as usize] {
                        head.phase += 1;
                        head.phase_time = head.age;
                    }
                }
                2 => {
                    if elapsed > self.lives[2] {
                        head.phase = 9;
                        head.phase_time = head.age;
                    }
                }
                _ => head.restart(self.anchor, self.scale[0]),
            }
            if (head.phase as i32) <= 3 {
                if let Some(trail) = head.trails.iter_mut().find(|trail| trail.phase == 0) {
                    trail.position[..2].copy_from_slice(&head.position[..2]);
                    trail.scale = head.scale;
                    trail.alpha = 0;
                    trail.phase = 1;
                    trail.age = 0;
                }
            }
            for trail in &mut head.trails {
                if trail.phase != 0 {
                    trail.position[..2].copy_from_slice(&head.position[..2]);
                    trail.alpha = integer(fade(10., 0., 40., trail.age as f32)) as u32 & 255;
                    trail.age = trail.age.wrapping_add(1);
                    if trail.age >= 40 {
                        trail.alpha = 0;
                    }
                }
            }
            for (i, value) in offset.into_iter().enumerate() {
                head.position[i] += value;
            }
            head.age = head.age.wrapping_add(integer(delta as f32));
        }
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        for (index, head) in self.heads.iter().enumerate() {
            for particle in 0..=head.trails.len() {
                let (at, position, scale, alpha) = if particle == 0 {
                    (index, head.position, head.scale, head.alpha)
                } else {
                    let trail = &head.trails[particle - 1];
                    (
                        self.heads.len() + index * head.trails.len() + particle - 1,
                        trail.position,
                        trail.scale,
                        trail.alpha,
                    )
                };
                let mut matrix = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
                matrix[0] = scale[0];
                matrix[5] = scale[1];
                matrix[12..15].copy_from_slice(&position);
                for (i, vertex) in host.quads[at].iter_mut().enumerate() {
                    let point = transform_vector(
                        [
                            (i & 1) as f32 * self.image[0] - self.image[0] * 0.5,
                            (i >> 1) as f32 * self.image[1] - self.image[1] * 0.5,
                            0.,
                            1.,
                        ],
                        matrix,
                    );
                    vertex.position.copy_from_slice(&point[..3]);
                    vertex.rhw = point[3];
                    vertex.color = 0xffffff | (alpha << 24);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ParticleMesh, Projection};

    #[test]
    fn rings_keep_activation_fades_trail_aging_and_respawn_geometry() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(20, [64., 128.]).unwrap();
        for (i, value) in [5f32, 640., 360., 1., 800., 50., 50., 20., 120., 220.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Hadou::new(&host, [64., 128.]).unwrap();
        for step in 1u32..400 {
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
                1 => Some([1, 0, 0, 1, 0x3c343958, 0x3c23d70a, 0, 1, 1, 10]),
                7 => Some([6, 0, 0, 1, 0x3c343958, 0x3c23d70a, 15, 7, 1, 8]),
                40 => Some([59, 21, 1, 1, 0x3c343958, 0x3c23d70a, 34, 40, 1, 0]),
                159 => Some([241, 142, 2, 4, 0x3c343958, 0x3c23d70a, 0, 159, 1, 0]),
                300 => Some([97, 25, 1, 4, 0x3c23d70a, 0x3c23d70a, 20, 62, 1, 0]),
                399 => Some([249, 150, 2, 4, 0x3c23d70a, 0x3c23d70a, 0, 161, 1, 0]),
                _ => None,
            };
            if let Some(expected) = expected {
                let h = &effect.heads[0];
                let t = &h.trails[0];
                assert_eq!(
                    [
                        h.age as u32,
                        h.phase_time as u32,
                        h.phase,
                        effect.visible,
                        t.scale[0].to_bits(),
                        t.scale[1].to_bits(),
                        h.alpha,
                        t.age as u32,
                        t.phase,
                        t.alpha,
                    ],
                    expected,
                    "step {step}"
                );
            }
            if step == 159 {
                let h = &effect.heads[0];
                assert_eq!(h.position.map(f32::to_bits), [0x441f3000, 0x43a8c000, 0]);
                assert_eq!(
                    h.trails[0].position.map(f32::to_bits),
                    [0x441f3000, 0x43b04000, 0x3f800000]
                );
            }
            if step == 40 {
                effect.write_mesh(&mut host);
                host.depth_offset = 5000.;
                let mesh = ParticleMesh::new(&host, None, 59000.).unwrap();
                for (index, expected) in [
                    (
                        0,
                        [0x44046862, 0x430482c8, 0x3f6c4ec5, 0x3a83126f, 0x22ffffff],
                    ),
                    (
                        4,
                        [0x441fe973, 0x43b3adff, 0x3f6c4dc3, 0x3a833405, 0x00ffffff],
                    ),
                ] {
                    let q = mesh.projected_quad(index, &Projection::default()).unwrap();
                    assert_eq!(
                        [
                            q[0].position[0].to_bits(),
                            q[0].position[1].to_bits(),
                            q[0].position[2].to_bits(),
                            q[0].rhw.to_bits(),
                            q[0].color
                        ],
                        expected
                    );
                    assert_eq!([q[0].uv, q[3].uv], [[0., 0.], [1., 1.]]);
                }
            }
        }
    }

    #[test]
    fn incomplete_groups_stay_untouched_and_zero_intervals_are_rejected() {
        let mut host = ParticleHost {
            surface_size: [1280, 720],
            ..Default::default()
        };
        host.reset_quads(20, [64., 128.]).unwrap();
        for (i, value) in [6f32, 901., 176., 86., 15., 3.9, 127.8, 2.9, 7.8, 10.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let original = host.quads[18..].to_vec();
        let mut effect = Hadou::new(&host, [64., 128.]).unwrap();
        effect.update(0, [650., 380.]);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[18..], original);
        assert_eq!(effect.visible, 1);
        host.parameters[5] = 0f32.to_bits();
        assert!(Hadou::new(&host, [64., 128.]).is_err());
        host.parameters[0] = 0f32.to_bits();
        host.parameters[5] = (-2f32).to_bits();
        let mut effect = Hadou::new(&host, [64., 128.]).unwrap();
        assert_eq!(effect.heads.len(), 20);
        assert!(effect.heads.iter().all(|h| h.trails.is_empty()));
        effect.update(0, [640., 360.]);
        effect.update(0, [640., 360.]);
        assert_eq!(effect.visible, 2);
        host.reset_quads(0, [64., 128.]).unwrap();
        let mut effect = Hadou::new(&host, [64., 128.]).unwrap();
        effect.update(0, [640., 360.]);
        effect.write_mesh(&mut host);
        assert_eq!(effect.visible, 0);
    }
}
