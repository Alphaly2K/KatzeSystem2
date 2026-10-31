use super::Interpreter;
use crate::{Error, Result, read, text};

pub(super) fn float32(source: &[u8]) -> u32 {
    let start = source
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t'..=b'\r'))
        .unwrap_or(source.len());
    let source = &source[start..];
    let (negative, source) = match source.first() {
        Some(b'-') => (true, &source[1..]),
        Some(b'+') => (false, &source[1..]),
        _ => (false, source),
    };
    let sign = if negative { 0x8000_0000 } else { 0 };
    if source
        .get(..3)
        .is_some_and(|s| s.eq_ignore_ascii_case(b"inf"))
    {
        return sign | f32::INFINITY.to_bits();
    }
    if source
        .get(..3)
        .is_some_and(|s| s.eq_ignore_ascii_case(b"nan"))
    {
        return sign | 0x7fff_ffff;
    }
    let hexadecimal = source.starts_with(b"0x") || source.starts_with(b"0X");
    let mut p = if hexadecimal { 2 } else { 0 };
    let start = p;
    let mut digits = 0;
    let mut fraction = 0i64;
    let mut dot = false;
    let mut mantissa = 0u64;
    let mut stored = 0;
    let mut dropped = 0i64;
    let mut sticky = false;
    while let Some(&b) = source.get(p) {
        if b == b'.' && !dot {
            dot = true;
        } else if let Some(n) = (b as char)
            .to_digit(if hexadecimal { 16 } else { 10 })
            .filter(|_| b.is_ascii())
        {
            digits += 1;
            if dot {
                fraction += 1;
            }
            if hexadecimal && (stored > 0 || n != 0) {
                if stored < 16 {
                    mantissa = (mantissa << 4) | n as u64;
                    stored += 1;
                } else {
                    dropped += 1;
                    sticky |= n != 0;
                }
            }
        } else {
            break;
        }
        p += 1;
    }
    if digits == 0 {
        return 0;
    }
    let number_end = p;
    let mut exponent = 0i64;
    let marker = if hexadecimal { b'p' } else { b'e' };
    if source
        .get(p)
        .is_some_and(|b| b.to_ascii_lowercase() == marker)
    {
        p += 1;
        let minus = source.get(p) == Some(&b'-');
        if matches!(source.get(p), Some(b'+' | b'-')) {
            p += 1;
        }
        let first = p;
        while let Some(&b) = source.get(p).filter(|b| b.is_ascii_digit()) {
            exponent = (exponent * 10 + (b - b'0') as i64).min(1_000_000_000);
            p += 1;
        }
        if first == p {
            p = number_end;
        }
        if minus {
            exponent = -exponent;
        }
    }
    let value = if hexadecimal {
        let scale = exponent + 4 * (dropped - fraction);
        let high_bit = 63 - mantissa.leading_zeros() as i64;
        if mantissa == 0 || scale + high_bit < -160 {
            0.0
        } else if scale + high_bit > 128 {
            f64::INFINITY
        } else {
            // 保留舍入位之后的非零信息，再按原 CRT 的 double → float 路径转换。
            let n = (mantissa | u64::from(sticky)) as f64;
            n * 2.0f64.powi(scale as i32)
        }
    } else {
        std::str::from_utf8(&source[start..p])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (value as f32).to_bits() | sign
}

pub(super) fn format(vm: &Interpreter, address: u32, depth: usize) -> Result<Vec<u8>> {
    if depth >= 32 {
        return Err(Error::new(vm.pc, "KCS 格式字符串引用过深"));
    }
    let source = vm.read_string(address)?;
    let mut output = Vec::new();
    let mut p = 0;
    while p < source.len() {
        let b = source[p];
        if matches!(b, 0x81..=0x9f | 0xe0..=0xfc) && p + 1 < source.len() {
            output.extend(&source[p..p + 2]);
            p += 2;
        } else if source[p..].starts_with(b"%%") {
            output.push(b'%');
            p += 2;
        } else if b == b'%' {
            let token = source[p + 1..]
                .iter()
                .position(|&b| b == b'[')
                .map(|i| p + 1 + i);
            let parsed = token.and_then(|bracket| {
                let scope = *source.get(bracket + 1)?;
                if !matches!(scope, b'G' | b'L' | b'H') {
                    return None;
                }
                let mut end = bracket + 2;
                let mut offset = 0u32;
                while let Some(&b) = source.get(end).filter(|b| b.is_ascii_digit()) {
                    offset = offset.wrapping_mul(10).wrapping_add((b - b'0') as u32);
                    end += 1;
                }
                if source.get(end) != Some(&b']') {
                    return None;
                }
                Some((
                    bracket,
                    end + 1,
                    match scope {
                        b'L' => offset.wrapping_add(vm.frame_base as u32),
                        b'H' => offset | 0x8000_0000,
                        _ => offset,
                    },
                ))
            });
            if let Some((bracket, end, value_address)) = parsed {
                let spec = text::ascii(&source[p + 1..bracket])?;
                let conversion = spec
                    .as_bytes()
                    .last()
                    .copied()
                    .ok_or_else(|| Error::new(vm.pc, "KCS 格式转换为空"))?;
                let text = if conversion == b's' {
                    format(vm, value_address, depth + 1)?
                } else {
                    let n = read::u32_at(vm.read(value_address, 4)?, 0)?;
                    match conversion {
                        b'd' | b'i' => (n as i32).to_string().into_bytes(),
                        b'u' => n.to_string().into_bytes(),
                        b'x' => format!("{n:x}").into_bytes(),
                        b'X' => format!("{n:X}").into_bytes(),
                        b'c' => vec![n as u8],
                        b'f' => {
                            let precision = spec[..spec.len() - 1]
                                .split_once('.')
                                .map(|(_, p)| field(p))
                                .transpose()?
                                .unwrap_or(6);
                            let f = f32::from_bits(n);
                            if f.is_nan() {
                                if f.is_sign_negative() {
                                    b"-nan(ind)".to_vec()
                                } else {
                                    b"nan".to_vec()
                                }
                            } else if f.is_infinite() {
                                if f.is_sign_negative() {
                                    b"-inf".to_vec()
                                } else {
                                    b"inf".to_vec()
                                }
                            } else {
                                format!("{:.precision$}", f as f64).into_bytes()
                            }
                        }
                        _ => return Err(Error::new(vm.pc, "未支持的 KCS 格式转换")),
                    }
                };
                output.extend(pad(spec, text)?);
                p = end;
            } else {
                output.push(b);
                p += 1;
            }
        } else {
            output.push(b);
            p += 1;
        }
        if output.len() > read::MAX_BYTES {
            return Err(Error::new(vm.pc, "KCS 格式输出超过上限"));
        }
    }
    if let Some(end) = output.iter().position(|&b| b == 0) {
        output.truncate(end);
    }
    Ok(output)
}

pub(super) fn compare(left: &[u8], right: &[u8], insensitive: bool) -> i32 {
    for i in 0..=left.len().min(right.len()) {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        if a != b {
            let difference = if insensitive {
                a.to_ascii_lowercase() as i32 - b.to_ascii_lowercase() as i32
            } else {
                a as i8 as i32 - b as i8 as i32
            };
            if difference != 0 {
                return difference;
            }
        }
    }
    0
}

pub(super) fn append_raw(vm: &mut Interpreter, target: u32, source: u32) -> Result<()> {
    let length = vm.read_string(source)?.len() + 1;
    let end = target.wrapping_add(vm.read_string(target)?.len() as u32);
    // 原入口先按 DWORD、再按单字节向前复制，重叠区不能先整体快照。
    let mut offset = 0;
    while offset < length {
        let width = if length - offset >= 4 { 4 } else { 1 };
        let mut bytes = [0; 4];
        bytes[..width].copy_from_slice(vm.read(source.wrapping_add(offset as u32), width)?);
        vm.write(end.wrapping_add(offset as u32), &bytes[..width])?;
        offset += width;
    }
    let end = target.wrapping_add(vm.read_string(target)?.len() as u32);
    vm.write(end, &[0, 0])
}

fn field(s: &str) -> Result<usize> {
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<usize>()
        .ok()
        .filter(|&n| n <= 65536)
        .ok_or_else(|| Error::new(0, "KCS 格式宽度或精度无效"))
}

fn pad(spec: &str, mut value: Vec<u8>) -> Result<Vec<u8>> {
    let conversion = spec.as_bytes()[spec.len() - 1];
    let prefix = &spec[..spec.len() - 1];
    let (width, precision) = prefix
        .split_once('.')
        .map_or((prefix, None), |(a, b)| (a, Some(b)));
    let width = field(width)?;
    if conversion == b's' {
        if let Some(p) = precision {
            value.truncate(field(p)?);
        }
    } else if conversion != b'f' && precision.is_some() {
        return Err(Error::new(0, "KCS 整数格式精度尚未实现"));
    }
    if value.len() >= width {
        return Ok(value);
    }
    let zero = prefix.starts_with('0') && !matches!(conversion, b's' | b'c');
    let mut out = vec![if zero { b'0' } else { b' ' }; width - value.len()];
    if zero && value.first() == Some(&b'-') {
        value.remove(0);
        out.insert(0, b'-');
    }
    out.extend(value);
    Ok(out)
}

pub(super) fn multi(vm: &Interpreter, mut address: u32, index: i32) -> Result<&[u8]> {
    for _ in 0..index {
        let word = vm.read_string(address)?;
        if word.is_empty() {
            return Ok(word);
        }
        address = address.wrapping_add(word.len() as u32 + 1);
    }
    vm.read_string(address)
}

pub(super) fn join(vm: &Interpreter, mut address: u32, first: i32, last: i32) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for index in 0..=last {
        let word = vm.read_string(address)?;
        if word.is_empty() {
            break;
        }
        if index >= first {
            output.extend(word);
            output.push(b' ');
            if output.len() > read::MAX_BYTES {
                return Err(Error::new(vm.pc, "KCS 词项拼接超过上限"));
            }
        }
        address = address.wrapping_add(word.len() as u32 + 1);
    }
    Ok(output)
}

pub(super) fn split(source: &[u8], separators: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut p = 0;
    while p < source.len() {
        let b = source[p];
        if separators.contains(&b) {
            if output.last().is_some_and(|&b| b != 0) {
                output.push(0);
            }
            p += 1;
        } else {
            let width = if matches!(b, 0x81..=0x9f | 0xe0..=0xfc) && p + 1 < source.len() {
                2
            } else {
                1
            };
            output.extend(&source[p..p + width]);
            p += width;
        }
    }
    output.extend([0, 0]);
    output
}
