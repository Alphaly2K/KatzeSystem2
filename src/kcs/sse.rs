// 归一化和求逆使用基线 SSE 的 12 位估计值，再执行原有的 Newton 修正。
const ESTIMATES: &[u8; 24576] = include_bytes!("sse-estimates.bin");

fn estimate(index: usize) -> u32 {
    0x3f00_0000
        | ((u16::from_le_bytes([ESTIMATES[index * 2], ESTIMATES[index * 2 + 1]]) as u32) << 11)
}

pub(super) fn reciprocal(value: f32) -> f32 {
    let bits = value.to_bits();
    let sign = bits & 0x8000_0000;
    let exponent = (bits >> 23) & 255;
    let fraction = bits & 0x007f_ffff;
    let result = match exponent {
        0 => 0x7f80_0000,
        255 if fraction != 0 => (bits & 0x7fff_ffff) | 0x0040_0000,
        255 => 0,
        _ => {
            let scaled =
                estimate((fraction >> 11) as usize) as i64 + (127 - exponent as i64) * 0x0080_0000;
            if scaled < 0x0080_0000 {
                0
            } else {
                scaled as u32
            }
        }
    };
    f32::from_bits(result | sign)
}

pub(super) fn reciprocal_sqrt(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 255;
    let fraction = bits & 0x007f_ffff;
    let result = if exponent == 255 && fraction != 0 {
        bits | 0x0040_0000
    } else if exponent == 0 {
        (bits & 0x8000_0000) | 0x7f80_0000
    } else if bits >> 31 != 0 {
        0xffc0_0000
    } else if exponent == 255 {
        0
    } else {
        let parity = usize::from(exponent & 1 == 0);
        let scaled = estimate(4096 + parity * 4096 + (fraction >> 11) as usize) as i64
            + ((127 + parity as i64 - exponent as i64) / 2) * 0x0080_0000;
        scaled as u32
    };
    f32::from_bits(result)
}

/// 四元数入口采用 SSE 多项式，和逐个调用 x87 三角函数的舍入不同。
pub(super) fn sin_cos(value: f32) -> (f32, f32) {
    let bits = value.to_bits();
    let sign = bits & 0x8000_0000;
    let magnitude = f32::from_bits(bits & 0x7fff_ffff);
    let rounded = magnitude * f32::from_bits(0x3f22_f983) + 8_388_608.0;
    let turns = rounded - 8_388_608.0;
    let mut reduced = magnitude;
    for coefficient in [0x3fc9_0000, 0x39fd_a000, 0x33a2_2000, 0x2c34_611a] {
        reduced -= turns * f32::from_bits(coefficient);
    }
    let square = reduced * reduced;
    let sine = (((f32::from_bits(0xb94c_a1f0) * square + f32::from_bits(0x3c08_839d)) * square
        + f32::from_bits(0xbe2a_aaa3))
        * square
        + 1.0)
        * reduced;
    let cosine = (((f32::from_bits(0x37cc_f5ce) * square + f32::from_bits(0xbab6_061a)) * square
        + f32::from_bits(0x3d2a_aaa5))
        * square
        - 0.5)
        * square
        + 1.0;
    let quadrant = rounded.to_bits() & 0x007f_ffff;
    let odd = quadrant & 1;
    let sine_sign = ((quadrant.wrapping_sub(odd) & 2) << 30) ^ sign;
    let cosine_sign = (quadrant.wrapping_add(odd) & 2) << 30;
    let (sine, cosine) = if odd == 0 {
        (sine, cosine)
    } else {
        (cosine, sine)
    };
    (
        f32::from_bits(sine.to_bits() ^ sine_sign),
        f32::from_bits(cosine.to_bits() ^ cosine_sign),
    )
}
