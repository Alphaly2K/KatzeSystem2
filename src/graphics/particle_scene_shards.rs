use crate::kcs::ParticleHost;

const FACES: [[u8; 4]; 6] = [
    [4, 5, 6, 7],
    [0, 4, 2, 6],
    [1, 0, 3, 2],
    [5, 1, 7, 3],
    [0, 1, 4, 5],
    [6, 7, 2, 3],
];
const DEPTH_SHIFTS: [f32; 6] = [0.07, 0.03, 0.02, 0.04, 0.05, 0.06];

fn corners(index: usize) -> [[f32; 3]; 4] {
    FACES[index % FACES.len()].map(|corner| {
        std::array::from_fn(|axis| {
            if corner & (1 << axis) == 0 {
                -100.
            } else {
                100.
            }
        })
    })
}

fn difference(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f64 {
    f64::from(a[0]) * f64::from(b[0])
        + f64::from(a[1]) * f64::from(b[1])
        + f64::from(a[2]) * f64::from(b[2])
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (dot(v, v) as f32).sqrt();
    if length > 0. {
        v.map(|n| n / length)
    } else {
        v
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let (a, b) = (a.map(f64::from), b.map(f64::from));
    [
        (a[1] * b[2] - a[2] * b[1]) as f32,
        (a[2] * b[0] - a[0] * b[2]) as f32,
        (a[0] * b[1] - a[1] * b[0]) as f32,
    ]
}

fn integer(value: f64) -> i32 {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&value) {
        value as i64 as i32
    } else {
        0
    }
}

fn lerp(a: f32, b: f32, first: f32, last: f32, time: f32) -> f32 {
    if time <= first {
        a
    } else if time >= last {
        b
    } else {
        ((time - first) / (last - first)) * (b - a) + a
    }
}

fn rotation([x, y, z]: [f32; 3], scale: f32) -> [[f32; 3]; 3] {
    let [[sx, cx], [sy, cy], [sz, cz]] = [x, y, z].map(|angle| {
        let angle = (f64::from(integer(f64::from(angle) * 1000.)) / 1000.) as f32;
        let (s, c) = f64::from(angle).sin_cos();
        [s as f32, c as f32]
    });
    let mut matrix = [[scale, 0., 0.], [0., scale, 0.], [0., 0., scale]];
    for rows in [
        [[1., 0., 0.], [0., cx, sx], [0., -sx, cx]],
        [[cy, 0., -sy], [0., 1., 0.], [sy, 0., cy]],
        [[cz, sz, 0.], [-sz, cz, 0.], [0., 0., 1.]],
    ] {
        matrix = rows.map(|row| {
            std::array::from_fn(|i| dot(row, [matrix[0][i], matrix[1][i], matrix[2][i]]) as f32)
        });
    }
    matrix
}

#[derive(Clone, Copy)]
enum Phase {
    FadeIn,
    Hold,
    Pause,
    FadeOut,
    Finished,
}

struct Group {
    position: [f32; 3],
    direction: [f32; 3],
    scale: f32,
    spin: f32,
    angle: [f32; 3],
    hold: i32,
    age: i32,
    phase_start: i32,
    phase: Phase,
}

impl Group {
    fn new([x, y, target_x, target_y, scale, spin, hold]: [f32; 7]) -> Self {
        Self {
            position: [x, y, 0.],
            direction: normalize(difference([target_x, target_y, 0.], [x, y, 0.]))
                .map(|n| (f64::from(n) * 12.) as f32),
            scale: (f64::from(scale) / 1000.) as f32,
            spin: (f64::from(spin) / 100.) as f32,
            angle: [0.628_318_55, std::f32::consts::FRAC_PI_2, 0.],
            hold: integer(hold.into()),
            age: 0,
            phase_start: 0,
            phase: Phase::FadeIn,
        }
    }

    fn advance(&mut self, delta: i32) -> (Option<u8>, [[f32; 3]; 3]) {
        let angle = lerp(
            std::f32::consts::FRAC_PI_2,
            0.,
            0.,
            self.hold as f32,
            self.age as f32,
        );
        let speed = f64::from(f64::from(angle).sin() as f32);
        for (position, direction) in self.position.iter_mut().zip(self.direction) {
            *position = (f64::from(*position) + speed * f64::from(direction)) as f32;
        }
        self.angle[1] = (f64::from(self.angle[1]) + 2. * f64::from(self.spin)) as f32;
        self.angle[2] += self.spin;
        let age = self.age;
        let time = age.wrapping_sub(self.phase_start);
        let (alpha, next) = match self.phase {
            Phase::FadeIn => (
                Some(lerp(0., 255., 0., 5., age as f32)),
                (age > 5).then_some(Phase::Hold),
            ),
            Phase::Hold => (Some(255.), (time > self.hold).then_some(Phase::Pause)),
            Phase::Pause => (Some(255.), (time > 10).then_some(Phase::FadeOut)),
            Phase::FadeOut => (
                Some(lerp(255., 10., 0., 10., time as f32)),
                (time > 10).then_some(Phase::Finished),
            ),
            Phase::Finished => (None, None),
        };
        if let Some(next) = next {
            self.phase = next;
            self.phase_start = age;
        }
        self.age = age.wrapping_add(delta);
        (
            alpha.map(|n| integer(n.into()) as u8),
            rotation(self.angle, self.scale),
        )
    }
}

pub(super) struct SceneShards([Group; 3]);

impl SceneShards {
    pub(super) fn new(host: &ParticleHost) -> Self {
        let p = host.parameters.map(f32::from_bits);
        Self(std::array::from_fn(|i| {
            Group::new(p[i * 7..i * 7 + 7].try_into().unwrap())
        }))
    }

    pub(super) fn depth_shifts(&self) -> &[f32] {
        &DEPTH_SHIFTS
    }

    pub(super) fn initialize_mesh(&self, host: &mut ParticleHost) {
        for (i, quad) in host.quads.iter_mut().enumerate() {
            for (vertex, position) in quad.iter_mut().zip(corners(i)) {
                vertex.position = position;
                vertex.color = 0x00ff_ffff;
            }
        }
    }

    pub(super) fn update(&mut self, delta: u32, host: &mut ParticleHost) {
        let delta = integer(f64::from(delta as f32));
        for (group, quads) in self.0.iter_mut().zip(host.quads.chunks_mut(FACES.len())) {
            let (alpha, matrix) = group.advance(delta);
            let toward_light = normalize(difference([640., 360., 970.], group.position));
            for (i, quad) in quads.iter_mut().enumerate() {
                let positions = corners(i).map(|corner| {
                    std::array::from_fn(|axis| {
                        (dot(corner, [matrix[0][axis], matrix[1][axis], matrix[2][axis]])
                            + f64::from(group.position[axis])) as f32
                    })
                });
                let [a, b, c, _] = positions;
                let normal = normalize(cross(difference(b, a), difference(c, a)));
                let shade =
                    (f64::from(normal[0]) + f64::from(normal[1]) - f64::from(normal[2])) as f32;
                let gray = integer(lerp(255., 60., -1., 1., shade).into()) as u8;
                let facing = dot(toward_light, normal) as f32;
                for (vertex, position) in quad.iter_mut().zip(positions) {
                    let alpha = if facing <= 0. {
                        0
                    } else {
                        alpha.unwrap_or((vertex.color >> 24) as u8)
                    };
                    vertex.position = position;
                    vertex.color = u32::from_be_bytes([alpha, gray, gray, gray]);
                }
            }
        }
    }
}
