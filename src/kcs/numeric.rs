use crate::{Error, Result};

pub(super) fn binary(op: u8, left: u32, right: u32, lf: u8, rf: u8, at: usize) -> Result<u32> {
    let flags = lf | rf;
    let signed = flags & 0x20 != 0;
    let base = match op {
        0x1b => 5,
        0x1c => 6,
        0x1d => 3,
        0x1e => 1,
        0x1f => 2,
        0x20 => 4,
        0x21 => 10,
        0x22 => 11,
        0x23 => 9,
        0x24 => 7,
        _ => op,
    };
    if (1..=4).contains(&base) && flags & 4 != 0 {
        // 复合乘加以右值为 SSE 目的操作数，NaN 的载荷优先级随之改变。
        return Ok(if matches!(op, 0x1d | 0x1e) {
            float(base, right, left)
        } else if op == 0x20 && f32::from_bits(right) == 0.0 {
            right
        } else {
            float(base, left, right)
        });
    }
    if (0x0f..=0x14).contains(&base) {
        return Ok(if lf & 4 != 0 {
            compare(base, f32::from_bits(left), f32::from_bits(right))
        } else if signed {
            compare(base, left as i32, right as i32)
        } else {
            compare(base, left, right)
        } as u32);
    }
    Ok(match base {
        1 => left.wrapping_add(right),
        2 => left.wrapping_sub(right),
        3 => left.wrapping_mul(right),
        4 | 5 if right == 0 => 0,
        4 | 5 if signed => {
            let (a, b) = (left as i32, right as i32);
            if a == i32::MIN && b == -1 {
                return Err(Error::new(at, "KCS 整数除法溢出"));
            }
            (if base == 4 { a / b } else { a % b }) as u32
        }
        // 基线的无符号复合除法保存 div 的余数寄存器。
        4 if op != 0x20 => left / right,
        4 | 5 => left % right,
        6 => left & right,
        7 => left | right,
        9 => left ^ right,
        10 => left.wrapping_shl(right),
        11 if op == 0x22 && signed => (left as i32).wrapping_shr(right) as u32,
        11 => left.wrapping_shr(right),
        13 => (left != 0 && right != 0) as u32,
        14 => (left != 0 || right != 0) as u32,
        _ => unreachable!(),
    })
}

fn compare<T: PartialOrd>(op: u8, a: T, b: T) -> bool {
    match op {
        0x0f => a > b,
        0x10 => a < b,
        0x11 => a >= b,
        0x12 => a <= b,
        0x13 => a == b,
        0x14 => a != b,
        _ => unreachable!(),
    }
}

pub(crate) fn float(op: u8, left: u32, right: u32) -> u32 {
    if op == 4 && f32::from_bits(right) == 0.0 {
        return 0;
    }
    float_ieee(op, left, right)
}

pub(super) fn float_ieee(op: u8, left: u32, right: u32) -> u32 {
    let a = f32::from_bits(left);
    let b = f32::from_bits(right);
    if a.is_nan() {
        return left | 0x0040_0000;
    }
    if b.is_nan() {
        return right | 0x0040_0000;
    }
    let n = match op {
        1 => a + b,
        2 => a - b,
        3 => a * b,
        4 => a / b,
        _ => unreachable!(),
    };
    if n.is_nan() { 0xffc0_0000 } else { n.to_bits() }
}

pub(super) fn convert(op: u8, n: u32) -> u32 {
    match op {
        0x36 => (n as f32).to_bits(),
        0x37 => (n as i32 as f32).to_bits(),
        0x38 => {
            let exponent = ((n >> 23) & 255) as i32 - 127;
            if !(0..55).contains(&exponent) {
                return 0;
            }
            let significand = (n & 0x007f_ffff) | 0x0080_0000;
            let low = if exponent >= 23 {
                significand.wrapping_shl((exponent - 23) as u32)
            } else {
                significand >> (23 - exponent)
            };
            if n >> 31 != 0 {
                low.wrapping_neg()
            } else {
                low
            }
        }
        0x39 => {
            let f = f32::from_bits(n);
            if !(-2147483648.0..2147483648.0).contains(&f) {
                0x8000_0000
            } else {
                (f as i32) as u32
            }
        }
        _ => unreachable!(),
    }
}

pub(super) fn native_math(index: u32, args: &[u32]) -> u32 {
    // 原宿主将 f32 升为 double 调用 CRT，然后舍入回 f32。
    let a = f32::from_bits(args[0]) as f64;
    let b = args.get(1).map_or(0.0, |&n| f32::from_bits(n) as f64);
    let value = match index {
        6 => a.sin(),
        7 => a.cos(),
        8 => a.tan(),
        9 => a.asin(),
        10 => a.acos(),
        11 => a.atan(),
        12 => a.atan2(b),
        13 => a.ceil(),
        14 => a.abs(),
        15 => a.floor(),
        16 => a % b,
        17 => a.sqrt(),
        18 => a.powf(b),
        _ => unreachable!(),
    };
    if value.is_nan() {
        if a.is_nan() {
            let bits = args[0] | 0x0040_0000;
            if index == 14 {
                bits & 0x7fff_ffff
            } else {
                bits
            }
        } else if b.is_nan() {
            args[1] | 0x0040_0000
        } else {
            0xffc0_0000
        }
    } else {
        (value as f32).to_bits()
    }
}

pub(super) fn native_triangle(input: f32, period: f32) -> f32 {
    let input = input.abs();
    let period = period.abs();
    let phase = if input > 0.0 { input % period } else { 0.0 };
    let quarter = period * 0.25;
    let part = if phase > 0.0 { phase % quarter } else { 0.0 } / quarter;
    let value = if quarter > phase {
        part
    } else if quarter + quarter > phase {
        1.0 - part
    } else if quarter * 3.0 > phase {
        -part
    } else {
        part - 1.0
    };
    if value.is_nan() && !input.is_nan() && !period.is_nan() {
        f32::from_bits(0xffc0_0000)
    } else {
        value
    }
}
