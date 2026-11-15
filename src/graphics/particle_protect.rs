use crate::kcs::ParticleHost;

fn integer(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
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

fn opacity(patch: usize, vertex: usize) -> f32 {
    // 中间偏右的面片保留独立的上下渐变。
    if patch == 13 {
        return if vertex < 2 { 255. } else { 128. };
    }
    let x = patch % 5 + (vertex & 1);
    let y = patch / 5 + (vertex >> 1);
    let x = x.min(5 - x);
    let y = y.min(5 - y);
    match (x, y) {
        (0, _) | (_, 0) => 0.,
        (1, 1) => 32.,
        (1, _) | (_, 1) => 128.,
        _ => 255.,
    }
}

fn velocity(value: f32, age: f32) -> f32 {
    let age = age.max(1.);
    if age as u32 >= 65 {
        return 0.;
    }
    let half = (90f64 * std::f32::consts::PI as f64 / 180.) as f32;
    let angle = linear(half, 0., 0., 65., age);
    ((angle as f64).cos() as f32 - (half as f64).cos() as f32) * (0. - value) + value
}

pub(super) struct Protect {
    groups: usize,
    anchor: [f32; 2],
    scale: f32,
    velocity: [f32; 2],
    age: f32,
    phase_time: f32,
    phase: u32,
    alpha: u32,
    mode: u32,
    updated: bool,
    uv: [[f32; 2]; 25],
}

impl Protect {
    pub(super) fn new(host: &ParticleHost) -> Self {
        let p = host.parameters.map(f32::from_bits);
        let mut scale = p[2] / 100.;
        if scale < 0.001 {
            scale = 0.001;
        }
        Self {
            groups: host.quads.len() / 26,
            anchor: [p[0], p[1]],
            scale,
            velocity: [p[3] / 10000., p[4] / 10000.],
            age: 0.,
            phase_time: 0.,
            phase: 0,
            alpha: 0,
            mode: u32::from(integer(p[5]) != 0),
            updated: false,
            uv: std::array::from_fn(|index| [index % 5, index / 5].map(|n| n as f32 / 5.)),
        }
    }

    pub(super) fn update(&mut self, delta: u32) {
        self.updated = true;
        if self.groups == 0 {
            return;
        }
        let time = self.age - self.phase_time;
        match self.phase {
            0 => {
                self.alpha = integer(linear(0., 255., 0., 60., time)) as u32;
                if time > 60. {
                    self.phase = 1;
                    self.phase_time = self.age;
                }
            }
            1 => {
                if time > 5. {
                    self.phase = 2;
                    self.phase_time = self.age;
                    self.mode = 1;
                }
            }
            2 => {
                self.mode = 1;
                self.alpha = integer(linear(255., 0., 0., 60., time)) as u32;
                if time > 60. {
                    self.phase = 9;
                    self.phase_time = self.age;
                }
            }
            _ => {}
        }
        // UV 按更新次数累加；零时间步仍然滚动，时间步只影响下一次取样。
        let delta_uv = self.velocity.map(|value| velocity(value, self.age));
        for uv in &mut self.uv {
            for (component, delta) in uv.iter_mut().zip(delta_uv) {
                *component += delta;
            }
        }
        self.age += integer(delta as f32) as f32;
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        host.render_mode = self.mode;
        host.texture_address = [if self.updated {
            super::TextureAddress::Wrap
        } else {
            super::TextureAddress::Mirror
        }; 2];
        for group in 0..self.groups {
            for vertex in &mut host.quads[group] {
                vertex.color &= 0xffffff;
            }
            for patch in 0..25 {
                let positions: [[f32; 3]; 4] = std::array::from_fn(|vertex| {
                    let x = (patch % 5 + (vertex & 1)) as f32;
                    let y = (patch / 5 + (vertex >> 1)) as f32;
                    [
                        (x * 100. - 250.) * self.scale + self.anchor[0],
                        (y * 100. - 250.) * self.scale + self.anchor[1],
                        -25. * ((x - 2.5).abs() - 0.5) + 0.,
                    ]
                });
                let a: [f32; 3] = std::array::from_fn(|i| positions[1][i] - positions[0][i]);
                let b: [f32; 3] = std::array::from_fn(|i| positions[2][i] - positions[0][i]);
                let mut normal = [
                    a[1] * b[2] - b[1] * a[2],
                    b[0] * a[2] - b[2] * a[0],
                    b[1] * a[0] - b[0] * a[1],
                ];
                let [x, y, z] = normal;
                let length = (((y * y + x * x) + z * z) as f64).sqrt() as f32;
                if length > 0. {
                    for value in &mut normal {
                        *value /= length;
                    }
                }
                let light = (normal[1] + normal[0]) - normal[2];
                let shade = if self.updated {
                    integer(linear(255., 60., -1., 1., light)) as u32 & 255
                } else {
                    255
                };
                let index = self.groups + group * 25 + patch;
                for (i, vertex) in host.quads[index].iter_mut().enumerate() {
                    vertex.position = if self.updated { positions[i] } else { [0.; 3] };
                    vertex.rhw = 1.;
                    let alpha = if self.updated {
                        self.alpha as f32 / 255.
                    } else {
                        1.
                    };
                    let alpha = integer(alpha * opacity(patch, i)) as u32 & 255;
                    vertex.color = (shade * 0x10101) | (alpha << 24);
                    vertex.uv = [
                        self.uv[patch][0] + (i & 1) as f32 * 0.2,
                        self.uv[patch][1] + (i >> 1) as f32 * 0.2,
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
    fn shield_keeps_strict_fades_uv_aging_and_additive_transition() {
        let mut host = ParticleHost::default();
        host.reset_quads(26, [64., 128.]).unwrap();
        for (i, value) in [390f32, 360., 170., -400., 400., 0.]
            .into_iter()
            .enumerate()
        {
            host.parameters[i] = value.to_bits();
        }
        let mut effect = Protect::new(&host);
        effect.write_mesh(&mut host);
        assert!(host.quads[0].iter().all(|v| v.color >> 24 == 0));
        assert!(host.quads[1].iter().all(|v| v.position == [0.; 3]));
        assert_eq!(host.quads[14].map(|v| v.color >> 24), [255, 255, 128, 128]);

        effect.update(60);
        effect.write_mesh(&mut host);
        assert_eq!(host.quads[1][0].position, [-35., -65., -50.]);
        assert_eq!(
            host.quads[1][0].uv.map(f32::to_bits),
            [3172983178, 1025499530]
        );
        assert_eq!(
            host.texture_address,
            [super::super::TextureAddress::Wrap; 2]
        );
        assert_eq!((effect.alpha, effect.phase), (0, 0));
        effect.update(0);
        assert_eq!((effect.alpha, effect.phase), (255, 0));
        effect.update(1);
        effect.update(5);
        assert_eq!((effect.phase_time, effect.phase, effect.mode), (61., 1, 0));
        effect.update(1);
        assert_eq!(effect.phase, 1);
        effect.update(0);
        effect.write_mesh(&mut host);
        assert_eq!(
            (effect.phase_time, effect.phase, host.render_mode),
            (67., 2, 1)
        );
        let uv = effect.uv;
        effect.update(60);
        assert_eq!(effect.alpha, 255);
        effect.update(1);
        assert_eq!((effect.alpha, effect.phase), (0, 2));
        effect.update(0);
        effect.write_mesh(&mut host);
        assert_eq!((effect.phase, effect.uv), (9, uv));
        assert!(host.quads.iter().flatten().all(|v| v.color >> 24 == 0));
    }

    #[test]
    fn incomplete_groups_stay_untouched_and_complete_groups_share_the_shield() {
        for count in [0, 1, 25, 53] {
            let mut host = ParticleHost::default();
            host.reset_quads(count, [64., 128.]).unwrap();
            let original = host.quads.clone();
            let mut effect = Protect::new(&host);
            assert_eq!(effect.scale, 0.001);
            effect.update(7);
            effect.write_mesh(&mut host);
            if count < 26 {
                assert_eq!(host.quads, original);
                assert_eq!(effect.age, 0.);
            } else {
                assert_eq!(host.quads[52], original[52]);
                assert_eq!(host.quads[2..27], host.quads[27..52]);
                assert_eq!(effect.age, 7.);
            }
        }
    }
}
