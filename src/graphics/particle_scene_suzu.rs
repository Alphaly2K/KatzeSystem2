use crate::kcs::ParticleHost;

pub(super) struct SceneSuzu {
    anchor: [f32; 2],
    image: [f32; 2],
    scale: f32,
    angles: [[f32; 2]; 2],
    spin: [f32; 4],
    velocity: f32,
    fade: f32,
    peak: f32,
    age: i32,
    alpha: u8,
    transformed: bool,
    depth_shifts: [f32; 4],
}

fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn sin_cos(radians: f32) -> [f32; 2] {
    let angle = ((f64::from(radians) * 1000.).trunc() / 1000.) as f32;
    let (sin, cos) = f64::from(angle).sin_cos();
    [sin as f32, cos as f32]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f64 {
    f64::from(a[0]) * f64::from(b[0])
        + f64::from(a[1]) * f64::from(b[1])
        + f64::from(a[2]) * f64::from(b[2])
}

impl SceneSuzu {
    pub(super) fn new(host: &ParticleHost, image: [f32; 2]) -> Self {
        let [x, y, scale, peak, pitch, yaw, velocity, fade, group, ..] =
            host.parameters.map(f32::from_bits);
        let angles = [pitch, yaw].map(|degrees| {
            sin_cos((f64::from(degrees) * f64::from(std::f32::consts::PI) / 180.) as f32)
        });
        Self {
            anchor: [x, y],
            image,
            scale: (f64::from(scale) / 100.) as f32,
            angles,
            spin: [0.; 4],
            velocity: (f64::from(velocity) / 100.) as f32,
            fade,
            peak,
            age: 0,
            alpha: 0,
            transformed: false,
            depth_shifts: if truncate(group) != 0 {
                [0., 0.01, 0.02, 0.03]
            } else {
                [0.001, 0.003, 0., 0.002]
            },
        }
    }

    pub(super) fn depth_shifts(&self) -> &[f32; 4] {
        &self.depth_shifts
    }

    pub(super) fn update(&mut self, delta: u32) {
        let age = self.age as f32;
        let alpha = if age <= 0. {
            0.
        } else if age >= self.fade {
            self.peak
        } else {
            (age / self.fade) * self.peak
        };
        self.alpha = truncate(alpha) as u8;
        self.age = self.age.wrapping_add(truncate(delta as f32));
        for (angle, direction) in self.spin.iter_mut().zip([2., 1., -2., -1.]) {
            *angle = (f64::from(*angle) + f64::from(self.velocity) * direction) as f32;
        }
        self.transformed = true;
    }

    pub(super) fn write_mesh(&self, host: &mut ParticleHost) {
        let [[sx, cx], [sy, cy]] = self.angles;
        let axes = [
            [cy, 0., sy],
            [sy * sx, cx, -cy * sx],
            [-sy * cx, sx, cy * cx],
        ];
        for (index, (quad, spin)) in host.quads.iter_mut().zip(self.spin).enumerate() {
            let [sz, cz] = sin_cos(spin);
            let transform: [[f32; 2]; 3] = std::array::from_fn(|axis| {
                let column = axes[axis];
                let weight = if axis == 2 { 1. } else { self.scale };
                let scaled = column.map(|v| v * weight);
                [
                    dot([cz, sz, 0.], scaled) as f32,
                    dot([-sz, cz, 0.], scaled) as f32,
                ]
            });
            for (corner, vertex) in quad.iter_mut().enumerate() {
                let u = (corner & 1) as f32;
                let v = (corner >> 1) as f32;
                let [x, y] = [self.image[0] * (u - 0.5), self.image[1] * (v - 0.5)];
                vertex.position = if self.transformed {
                    std::array::from_fn(|axis| {
                        let [across, down] = transform[axis];
                        (dot(
                            [x, y, 1.],
                            [across, down, self.anchor.get(axis).copied().unwrap_or(0.)],
                        )) as f32
                    })
                } else {
                    [self.anchor[0] + x, self.anchor[1] + y, 0.]
                };
                vertex.rhw = 1.;
                vertex.color = (u32::from(self.alpha) << 24) | 0xff_ffff;
                vertex.uv = [
                    ((index & 1) as f32 + u) * 0.5,
                    ((index >> 1) as f32 + v) * 0.5,
                ];
            }
        }
    }
}
