use super::{numeric, sse};

type V = [f32; 4];
fn scalar(op: u8, a: f32, b: f32) -> f32 {
    f32::from_bits(numeric::float(op, a.to_bits(), b.to_bits()))
}
fn mul(a: V, b: V) -> V {
    std::array::from_fn(|i| scalar(3, a[i], b[i]))
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| scalar(1, a[i], b[i]))
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| scalar(2, a[i], b[i]))
}
fn shuffle(a: V, b: V, order: u8) -> V {
    [
        a[(order & 3) as usize],
        a[((order >> 2) & 3) as usize],
        b[((order >> 4) & 3) as usize],
        b[(order >> 6) as usize],
    ]
}

pub(crate) fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let row = i / 4;
        let col = i % 4;
        let left = std::array::from_fn(|j| left[row * 4 + j]);
        let right = std::array::from_fn(|j| right[j * 4 + col]);
        // 最后一行以右矩阵为 SSE 目的操作数，保留其 NaN 载荷优先级。
        let p = if row == 3 {
            mul(right, left)
        } else {
            mul(left, right)
        };
        if row == 0 || row == 3 {
            scalar(1, scalar(1, p[0], p[1]), scalar(1, p[2], p[3]))
        } else {
            scalar(1, scalar(1, scalar(1, p[0], p[1]), p[2]), p[3])
        }
    })
}

pub(crate) fn transform(v: V, matrix: [f32; 16]) -> V {
    std::array::from_fn(|i| {
        let p = mul(v, std::array::from_fn(|j| matrix[j * 4 + i]));
        scalar(1, scalar(1, p[0], p[1]), scalar(1, p[2], p[3]))
    })
}

pub(crate) fn normalize4(v: V) -> V {
    let square = (v[0] * v[0] + v[1] * v[1]) + (v[2] * v[2] + v[3] * v[3]);
    let seed = sse::reciprocal_sqrt(square);
    let mut factor = (3.0 - square * (seed * seed)) * 0.5 * seed;
    if factor.is_nan() && !square.is_nan() {
        factor = f32::from_bits(0xffc0_0000);
    }
    mul(v, [factor; 4])
}

pub(crate) fn rotation_yaw_pitch_roll(yaw: f32, pitch: f32, roll: f32) -> [f32; 16] {
    let half_angle = |angle: f32| {
        let (s, c) = sin_cos(angle * 0.5);
        (f64::from(s), f64::from(c))
    };
    let (sy, cy) = half_angle(yaw);
    let (sp, cp) = half_angle(pitch);
    let (sr, cr) = half_angle(roll);
    let round = |n: f64| f64::from(n as f32);
    let x = round((cy * sp) * cr + (sy * cp) * sr);
    let y = round((sy * cp) * cr - (cy * sp) * sr);
    let z = round((cy * cp) * sr - (sy * sp) * cr);
    let w = round((cy * cp) * cr + (sy * sp) * sr);

    // 矩阵转换只在部分中间积写回单精度，最后一项对角线复用已舍入的差值。
    let (x2, y2, z2) = (2.0 * x, 2.0 * y, 2.0 * z);
    let (xx, xy, xz) = (round(x2 * x), round(y2 * x), round(z2 * x));
    let (yy, yz, zz) = (round(y2 * y), round(z2 * y), z2 * z);
    let (xw, yw, zw) = (x2 * w, y2 * w, z2 * w);
    [
        1.0 - yy - zz,
        xy + zw,
        xz - yw,
        0.0,
        xy - zw,
        1.0 - xx - zz,
        yz + xw,
        0.0,
        xz + yw,
        yz - xw,
        round(1.0 - xx) - yy,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
    .map(|n| n as f32)
}

pub(super) fn rotation_axis(axis: [f32; 3], angle: f32) -> [f32; 16] {
    let square = (axis[0] * axis[0] + axis[1] * axis[1]) + axis[2] * axis[2];
    let axis = if square.is_nan() || square < f32::from_bits(0x2880_0000) {
        [0.0; 3]
    } else {
        let seed = sse::reciprocal_sqrt(square);
        let factor = (3.0 - (seed * square) * seed) * (seed * 0.5);
        axis.map(|n| n * factor)
    };
    let [x, y, z] = axis.map(f64::from);
    let sine = (angle as f64).sin() as f32 as f64;
    let cosine = (angle as f64).cos() as f32 as f64;
    let t = (1.0 - cosine) as f32 as f64;
    let xy = (y * x * t) as f32 as f64;
    let yz = y * z * t;
    let xz = z * x * t;
    let xs = x * sine;
    let ys = y * sine;
    let zs = z * sine;
    [
        (x * x * t + cosine) as f32,
        (xy + zs) as f32,
        (xz - ys) as f32,
        0.0,
        (xy - zs) as f32,
        (y * y * t + cosine) as f32,
        (xs + yz) as f32,
        0.0,
        (ys as f32 as f64 + xz) as f32,
        (yz - xs) as f32,
        (z * z * t + cosine) as f32,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// 保留基线 SSE 的余子式运算顺序和倒数估计修正；奇异矩阵保持原值。
pub(super) fn inverse(m: [f32; 16]) -> Option<[f32; 16]> {
    let mut v2 = [m[2], m[3], m[6], m[7]];
    let mut v4 = [m[10], m[11], m[14], m[15]];
    let mut v3 = [m[8], m[9], m[12], m[13]];
    let mut v1 = [m[0], m[1], m[4], m[5]];
    let mut v5 = shuffle(v2, v4, 0x88);
    v4 = shuffle(v4, v2, 0xdd);
    v2 = mul(v4, v5);
    v2 = shuffle(v2, v2, 0xb1);
    let mut v6 = shuffle(v2, v2, 0x4e);
    let mut v7 = v3;
    v3 = shuffle(v3, v1, 0xdd);
    v1 = shuffle(v1, v7, 0x88);
    v7 = v3;
    v3 = mul(v3, v6);
    v6 = mul(v6, v1);
    let d0 = v6;
    v6 = v7;
    v7 = mul(v7, v2);
    v2 = mul(v2, v1);
    v3 = sub(v3, v7);
    v7 = mul(v6, v5);
    v5 = shuffle(v5, v5, 0x4e);
    v7 = shuffle(v7, v7, 0xb1);
    let c0 = v2;
    v2 = add(mul(v4, v7), v3);
    v3 = v7;
    v7 = shuffle(v7, v7, 0x4e);
    v3 = mul(v3, v1);
    let b0 = v3;
    v3 = mul(v4, v7);
    v7 = mul(v7, v1);
    v2 = sub(v2, v3);
    v3 = shuffle(v6, v6, 0x4e);
    v3 = mul(v3, v4);
    v3 = shuffle(v3, v3, 0xb1);
    let a0 = v7;
    v7 = v5;
    v5 = add(mul(v5, v3), v2);
    v2 = v3;
    v3 = shuffle(v3, v3, 0x4e);
    v2 = mul(v2, v1);
    let r90 = v4;
    v4 = v7;
    v7 = mul(v7, v3);
    v3 = mul(v3, v1);
    v5 = sub(v5, v7);
    v3 = sub(v3, v2);
    v2 = v1;
    v1 = mul(v1, v5);
    v3 = shuffle(v3, v3, 0x4e);
    v7 = v1;
    v1 = shuffle(v1, v1, 0x4e);
    let e0 = v5;
    v1 = add(v1, v7);
    v5 = v1;
    v1 = shuffle(v1, v1, 0xb1);
    v1[0] = f32::from_bits(numeric::float(1, v1[0].to_bits(), v5[0].to_bits()));
    let determinant = v1[0];
    v5 = mul(v6, v2);
    v5 = shuffle(v5, v5, 0xb1);
    v7 = v5;
    v5 = shuffle(v5, v5, 0x4e);
    let r80 = v4;
    v4 = r90;
    let r70 = v6;
    v6 = add(mul(v4, v7), v3);
    v3 = sub(mul(v4, v5), v6);
    v6 = mul(v4, v2);
    v6 = shuffle(v6, v6, 0xb1);
    let r60 = v5;
    v5 = r70;
    let r50 = v7;
    v7 = add(mul(v6, v5), v3);
    v3 = shuffle(v6, v6, 0x4e);
    let r40 = v4;
    v4 = v5;
    v5 = mul(v5, v3);
    let r30 = v4;
    v4 = v6;
    v6 = sub(v7, v5);
    v5 = shuffle(sub(d0, c0), sub(d0, c0), 0x4e);
    v7 = r80;
    v4 = mul(v4, v7);
    v3 = mul(v3, v7);
    v5 = sub(v5, v4);
    v2 = mul(v2, v7);
    v3 = add(v3, v5);
    v2 = shuffle(v2, v2, 0xb1);
    v4 = shuffle(v2, v2, 0x4e);
    v5 = r40;
    let r20 = v6;
    v6 = v5;
    v5 = mul(v5, v2);
    v6 = mul(v6, v4);
    v5 = add(v5, v3);
    v3 = v4;
    v4 = sub(v5, v6);
    v5 = shuffle(sub(a0, b0), sub(a0, b0), 0x4e);
    v6 = sub(mul(r50, v7), v5);
    v5 = r60;
    v7 = mul(v7, v5);
    v6 = sub(v6, v7);
    v5 = r30;
    v2 = mul(v2, v5);
    v5 = mul(v5, v3);
    v6 = sub(v6, v2);
    v2 = add(v5, v6);
    if determinant == 0.0 || determinant.is_nan() {
        return None;
    }
    let seed = sse::reciprocal(determinant);
    let factor = scalar(
        2,
        scalar(1, seed, seed),
        scalar(3, determinant, scalar(3, seed, seed)),
    );
    let rows = [
        mul(e0, [factor; 4]),
        mul(v4, [factor; 4]),
        mul(r20, [factor; 4]),
        mul([factor; 4], v2),
    ];
    Some(std::array::from_fn(|i| rows[i / 4][i % 4]))
}

pub(super) fn yaw_pitch_roll(angles: [f32; 3]) -> [f32; 16] {
    let [yaw, pitch, roll] = angles.map(|n| n * 0.5);
    let (sy, cy) = sse::sin_cos(yaw);
    let (sp, cp) = sse::sin_cos(pitch);
    let (sr, cr) = sse::sin_cos(roll);
    let q = [
        (cr * cy) * sp + (sr * sy) * cp,
        (cr * sy) * cp - (sr * cy) * sp,
        (sr * cy) * cp - (cr * sy) * sp,
        (cr * cy) * cp + (sr * sy) * sp,
    ];
    let [x, y, z, w] = q.map(f64::from);
    let xx = (2.0 * x * x) as f32 as f64;
    let yy = (2.0 * y * y) as f32 as f64;
    let xy = (2.0 * y * x) as f32 as f64;
    let xz = (2.0 * z * x) as f32 as f64;
    let yz = (2.0 * z * y) as f32 as f64;
    let zz = 2.0 * z * z;
    let xw = 2.0 * x * w;
    let yw = 2.0 * y * w;
    let zw = 2.0 * z * w;
    [
        ((1.0 - yy) - zz) as f32,
        (xy + zw) as f32,
        (xz - yw) as f32,
        0.0,
        (xy - zw) as f32,
        ((1.0 - xx) - zz) as f32,
        (yz + xw) as f32,
        0.0,
        (xz + yw) as f32,
        (yz - xw) as f32,
        ((1.0 - xx) as f32 as f64 - yy) as f32,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

pub(crate) fn sin_cos(angle: f32) -> (f32, f32) {
    if angle.is_nan() {
        let nan = f32::from_bits(angle.to_bits() | 0x0040_0000);
        (nan, nan)
    } else if angle.is_infinite() {
        let nan = f32::from_bits(0xffc0_0000);
        (nan, nan)
    } else {
        let bits = angle.to_bits();
        let magnitude = bits & 0x7fff_ffff;
        if magnitude >= 0x5f00_0000 {
            // FSINCOS 越过 2^63 时不压入第二个结果，随后读空栈得到负 indefinite。
            return (f32::from_bits(0xffc0_0000), angle);
        }
        let mut reduced = angle as f64;
        if magnitude >= 0x3f80_0000 {
            // 原 x87 用 66 位 π 约化；先做整数取余，避免大角度丢失低位。
            const PI: u128 = 0x3_243f_6a88_85a3_08d3;
            let exponent = (magnitude >> 23) as usize;
            let significand = ((magnitude & 0x7f_ffff) | 0x80_0000) as u128;
            let remainder = (significand << (exponent - 86)) % (2 * PI);
            let remainder = if remainder > PI {
                remainder as i128 - (2 * PI) as i128
            } else {
                remainder as i128
            };
            reduced = remainder as f64 / 18_446_744_073_709_551_616.0;
            if bits >> 31 != 0 {
                reduced = -reduced;
            }
        }
        (reduced.sin() as f32, reduced.cos() as f32)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn multiply_last_row_preserves_right_nan_priority() {
        let left = [f32::from_bits(0xffc0_1234); 16];
        let right = [f32::from_bits(0x7fc0_5678); 16];
        let result = super::multiply(left, right).map(f32::to_bits);
        assert_eq!(&result[..12], &[0xffc0_1234; 12]);
        assert_eq!(&result[12..], &[0x7fc0_5678; 4]);
    }
}
