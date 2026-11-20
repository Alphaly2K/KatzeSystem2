use crate::kcs::ieee_float_operation;

fn length(vector: [f32; 3]) -> f32 {
    let squares = vector.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
}

fn extended_distance(time: f32, duration: f32, length: f32) -> f32 {
    let quotient = time as f64 / duration as f64;
    let high = quotient * length as f64;
    let result = high as f32;
    let adjacent = if high > result as f64 {
        result.next_up()
    } else {
        result.next_down()
    };
    let midpoint = (result as f64 + adjacent as f64) * 0.5;
    if !high.is_finite() || (high - midpoint).abs() > high.abs() * f64::EPSILON {
        return result;
    }
    // 单精度中点需保留除法、乘法各自的 64 位有效数舍入方向。
    let parts = |value: f32| {
        let bits = value.to_bits() & 0x7fff_ffff;
        let exponent = (bits >> 23) as i32;
        if exponent == 0 {
            (bits as u128, -149)
        } else {
            (((bits & 0x7f_ffff) | 0x80_0000) as u128, exponent - 150)
        }
    };
    let rounded_ratio = |numerator: u128, denominator: u128| {
        let quotient = numerator / denominator;
        let remainder = (numerator % denominator) * 2;
        quotient
            + u128::from(remainder > denominator || (remainder == denominator && quotient & 1 != 0))
    };
    let (a, ae) = parts(time);
    let (b, be) = parts(duration);
    let (c, ce) = parts(length);
    let exponent = ((quotient.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    let significand = rounded_ratio(a << (ae - be - exponent + 63), b);
    let product = significand * c;
    let shift = (128 - product.leading_zeros()).saturating_sub(64);
    let wide = rounded_ratio(product, 1 << shift);
    let scale = f64::from_bits(((exponent - 63 + ce + shift as i32 + 1023) as u64) << 52);
    let midpoint_significand = (midpoint.abs() / scale) as u128;
    let direction = if high.is_sign_negative() {
        midpoint_significand.cmp(&wide)
    } else {
        wide.cmp(&midpoint_significand)
    };
    if direction.is_eq() {
        if result.to_bits() & 1 == 0 {
            result
        } else {
            adjacent
        }
    } else if (midpoint > result as f64 && direction.is_gt())
        || (midpoint < result as f64 && direction.is_lt())
    {
        adjacent
    } else {
        result
    }
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
            extended_distance(time, self.duration, self.length)
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

fn arithmetic(op: u8, left: f32, right: f32) -> f32 {
    f32::from_bits(ieee_float_operation(op, left.to_bits(), right.to_bits()))
}

pub(super) struct Path {
    pub(super) points: Vec<[f32; 3]>,
    pub(super) tangents: Vec<[f32; 3]>,
    pub(super) lengths: Vec<f32>,
    pub(super) total: f32,
    pub(super) duration: f32,
    extended_intermediates: bool,
}

impl Path {
    pub(super) fn new(points: Vec<[f32; 3]>, duration: f32, strength: f32) -> Self {
        Self::with_rounding(points, duration, strength, false, false)
    }

    pub(super) fn new_extended(points: Vec<[f32; 3]>, duration: f32, strength: f32) -> Self {
        Self::with_rounding(points, duration, strength, true, false)
    }

    pub(super) fn new_extended_relative(
        points: Vec<[f32; 3]>,
        duration: f32,
        strength: f32,
    ) -> Self {
        Self::with_rounding(points, duration, strength, true, true)
    }

    fn with_rounding(
        points: Vec<[f32; 3]>,
        duration: f32,
        strength: f32,
        extended_intermediates: bool,
        relative_strength: bool,
    ) -> Self {
        let lengths: Vec<_> = std::iter::once(0.)
            .chain(points.windows(2).map(|p| {
                let delta = std::array::from_fn(|j| p[1][j] - p[0][j]);
                if extended_intermediates {
                    let squares = delta.map(|n| n * n);
                    let sum = ((squares[0] as f64 + squares[1] as f64) + squares[2] as f64) as f32;
                    (sum as f64).sqrt() as f32
                } else {
                    length(delta)
                }
            }))
            .collect();
        let total = lengths.iter().copied().fold(0., |a, b| a + b);
        let strength = if relative_strength {
            total * strength
        } else {
            strength
        };
        let tangents = (0..points.len())
            .map(|i| {
                if duration == 0. {
                    [0.; 3]
                } else {
                    let delta = if i == 0 {
                        std::array::from_fn(|j| points[1][j] - points[0][j])
                    } else if i + 1 == points.len() {
                        std::array::from_fn(|j| points[i][j] - points[i - 1][j])
                    } else {
                        std::array::from_fn(|j| {
                            (points[i + 1][j] - points[i][j]) - (points[i - 1][j] - points[i][j])
                        })
                    };
                    let n = if extended_intermediates {
                        let squares = delta.map(|n| n as f64 * n as f64);
                        let sum = ((squares[0] + squares[1]) + squares[2]) as f32;
                        (sum as f64).sqrt() as f32
                    } else {
                        length(delta)
                    };
                    delta.map(|v| arithmetic(3, arithmetic(4, v, n), strength))
                }
            })
            .collect();
        Self {
            points,
            tangents,
            lengths,
            total,
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
            extended_distance(time, self.duration, self.total)
        } else {
            (time / self.duration) * self.total
        };
        let mut previous = 0f32;
        for i in 0..self.points.len() - 1 {
            let next = if self.extended_intermediates {
                previous as f64 + self.lengths[i + 1] as f64
            } else {
                (previous + self.lengths[i + 1]) as f64
            };
            if next > distance as f64 {
                let t = (distance - previous) / self.lengths[i + 1];
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
                let c = basis.map(|r| {
                    std::array::from_fn::<_, 3, _>(|j| {
                        if self.extended_intermediates {
                            (((self.points[i][j] as f64 * r[0] as f64
                                + self.points[i + 1][j] as f64 * r[1] as f64)
                                + self.tangents[i][j] as f64 * r[2] as f64)
                                + self.tangents[i + 1][j] as f64 * r[3] as f64)
                                as f32
                        } else {
                            ((self.points[i][j] * r[0] + self.points[i + 1][j] * r[1])
                                + self.tangents[i][j] * r[2])
                                + self.tangents[i + 1][j] * r[3]
                        }
                    })
                });
                return std::array::from_fn(|j| {
                    if self.extended_intermediates {
                        (((c[0][j] as f64 * cube as f64 + c[1][j] as f64 * square as f64)
                            + c[2][j] as f64 * t as f64)
                            + c[3][j] as f64) as f32
                    } else {
                        ((c[0][j] * cube + c[1][j] * square) + c[2][j] * t) + c[3][j]
                    }
                });
            }
            previous = next as f32;
        }
        *self.points.last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_distance_keeps_the_division_residual_at_single_precision_midpoints() {
        for (time, duration, length, expected) in [
            (130., 200., 1110770263, 0x41eb_4fd7),
            (130., 200., 1111656083, 0x41fc_e225),
            (11., 10., 1159911431, 0x4533_20d4),
            (41., 10., 1153398508, 0x45c4_404c),
        ] {
            let length = f32::from_bits(length);
            assert_eq!(
                extended_distance(time, duration, length).to_bits(),
                expected
            );
            assert_eq!(
                extended_distance(time, duration, -length).to_bits(),
                expected | 0x8000_0000
            );
        }
        assert_eq!(extended_distance(1., 2., 10.), 5.);
    }
}
