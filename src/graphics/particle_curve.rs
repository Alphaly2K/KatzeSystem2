use crate::kcs::ieee_float_operation;

fn length(vector: [f32; 3]) -> f32 {
    let squares = vector.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
}

pub(super) struct TwoPointPath {
    pub(super) points: [[f32; 3]; 2],
    pub(super) tangent: [f32; 3],
    pub(super) length: f32,
    pub(super) duration: f32,
    extended_intermediates: bool,
}

impl TwoPointPath {
    pub(super) fn new(points: [[f32; 3]; 2], duration: f32, strength: f32) -> Self {
        Self::with_rounding(points, duration, strength, false)
    }

    pub(super) fn new_extended(points: [[f32; 3]; 2], duration: f32, strength: f32) -> Self {
        Self::with_rounding(points, duration, strength, true)
    }

    fn with_rounding(
        points: [[f32; 3]; 2],
        duration: f32,
        strength: f32,
        extended_intermediates: bool,
    ) -> Self {
        let delta = std::array::from_fn(|i| points[1][i] - points[0][i]);
        let (length, tangent_length) = if extended_intermediates {
            // 段长先舍入平方，切线长度先求平方和，两者分别写回单精度。
            let squares = delta.map(|n| n as f64 * n as f64);
            let sum = ((squares[0] + squares[1]) + squares[2]) as f32;
            let tangent_length = (sum as f64).sqrt() as f32;
            let squares = delta.map(|n| n * n);
            let sum = ((squares[0] as f64 + squares[1] as f64) + squares[2] as f64) as f32;
            ((sum as f64).sqrt() as f32, tangent_length)
        } else {
            let length = length(delta);
            (length, length)
        };
        let tangent = if duration == 0. {
            [0.; 3]
        } else {
            delta.map(|n| {
                let quotient = ieee_float_operation(4, n.to_bits(), tangent_length.to_bits());
                f32::from_bits(ieee_float_operation(3, quotient, strength.to_bits()))
            })
        };
        Self {
            points,
            tangent,
            length,
            duration,
            extended_intermediates,
        }
    }

    pub(super) fn at(&self, mut time: f32) -> [f32; 3] {
        if time <= 0. {
            return self.points[0];
        }
        if time >= self.duration {
            time = self.duration;
        }
        let distance = if self.extended_intermediates {
            (time as f64 / self.duration as f64 * self.length as f64) as f32
        } else {
            (time / self.duration) * self.length
        };
        if self.length <= distance || distance.is_nan() {
            return self.points[1];
        }
        let t = (distance - 0.) / self.length;
        let square = t * t;
        let cube = if self.extended_intermediates {
            (t as f64 * t as f64 * t as f64) as f32
        } else {
            square * t
        };
        let basis = [
            [2., -2., 1., 1.],
            [-3., 3., -2., -1.],
            [0., 0., 1., 0.],
            [1., 0., 0., 0.],
        ];
        let coefficients = basis.map(|row| {
            std::array::from_fn::<_, 3, _>(|i| {
                if self.extended_intermediates {
                    (((self.points[0][i] as f64 * row[0] as f64
                        + self.points[1][i] as f64 * row[1] as f64)
                        + self.tangent[i] as f64 * row[2] as f64)
                        + self.tangent[i] as f64 * row[3] as f64) as f32
                } else {
                    ((self.points[0][i] * row[0] + self.points[1][i] * row[1])
                        + self.tangent[i] * row[2])
                        + self.tangent[i] * row[3]
                }
            })
        });
        std::array::from_fn(|i| {
            if self.extended_intermediates {
                (((coefficients[0][i] as f64 * cube as f64
                    + coefficients[1][i] as f64 * square as f64)
                    + coefficients[2][i] as f64 * t as f64)
                    + coefficients[3][i] as f64) as f32
            } else {
                ((coefficients[0][i] * cube + coefficients[1][i] * square) + coefficients[2][i] * t)
                    + coefficients[3][i]
            }
        })
    }
}
