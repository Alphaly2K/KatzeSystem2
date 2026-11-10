use crate::{Error, Result};
use std::collections::BTreeMap;

pub fn cp932(bytes: &[u8]) -> Result<String> {
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|s| s.into_owned())
        .ok_or_else(|| Error::new(0, "无效 CP932 文本"))
}

pub fn ascii(bytes: &[u8]) -> Result<&str> {
    if !bytes.is_ascii() {
        return Err(Error::new(0, "语法字段需要 ASCII"));
    }
    Ok(std::str::from_utf8(bytes).unwrap())
}

pub(crate) fn font_name(bytes: &[u8], offset: usize) -> Result<Vec<u16>> {
    let bytes = bytes.split(|&byte| byte == 0).next().unwrap();
    let mut name = Vec::new();
    let mut cursor = 0;
    while bytes.get(cursor).copied().unwrap_or(0) != 0 {
        let byte = bytes[cursor];
        let (unit, length) = if bytes.get(cursor..cursor + 2) == Some(&[255, 255]) {
            (u16::MAX, 2)
        } else {
            // 原读取器只核对中间续字节，逐项转换后保留首个 UTF-16 单元。
            let leading = byte.leading_ones();
            if leading == 1 {
                break;
            }
            let length = if leading >= 6 { 1 } else { leading.max(1) } as usize;
            if (1..length.saturating_sub(1))
                .any(|i| bytes.get(cursor + i).copied().unwrap_or(0) & 0xc0 != 0x80)
            {
                break;
            }
            if cursor + length > bytes.len() {
                return Err(Error::new(offset, "字体名 UTF-8 字符被截断"));
            }
            let mut chunk = [0; 5];
            for (i, value) in chunk[..length].iter_mut().enumerate() {
                *value = bytes.get(cursor + i).copied().unwrap_or(0);
            }
            let text = String::from_utf8_lossy(&chunk[..length]);
            let unit = text.encode_utf16().next().unwrap_or(0);
            (unit, length)
        };
        if name.len() == 63 {
            return Err(Error::new(offset, "字体名超过 63 个 UTF-16 单元"));
        }
        name.push(unit);
        cursor += length;
    }
    Ok(name)
}

pub fn expand(input: &[u8], strings: &BTreeMap<u32, Vec<u8>>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < input.len() {
        if let Some((length, index)) = input[p..].strip_prefix(b"$str").and_then(integer_prefix) {
            p += 4 + length;
            if let Some(s) = strings.get(&index) {
                out.extend(s);
            }
            if input.get(p) == Some(&b';') {
                p += 1;
            }
        } else {
            out.push(input[p]);
            p += 1;
        }
        if out.len() > crate::read::MAX_BYTES {
            return Err(Error::new(p, "字符串展开超过上限"));
        }
    }
    Ok(out)
}

pub(crate) fn integer_prefix(bytes: &[u8]) -> Option<(usize, u32)> {
    let mut p = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    if !bytes.get(p)?.is_ascii_digit() {
        return None;
    }
    let radix = if bytes[p..].starts_with(b"0x") || bytes[p..].starts_with(b"0X") {
        p += 2;
        16
    } else {
        10
    };
    let mut n = 0u32;
    while let Some(d) = bytes.get(p).and_then(|&b| (b as char).to_digit(radix)) {
        n = n.wrapping_mul(radix).wrapping_add(d);
        p += 1;
    }
    if radix == 10 && bytes.get(p) == Some(&b'.') {
        return None;
    }
    Some((
        p,
        if bytes.first() == Some(&b'-') {
            n.wrapping_neg()
        } else {
            n
        },
    ))
}

/// 按已编译 CST 的半角空格和圆括号规则提取词项。
pub fn arguments(input: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let mut word = Vec::new();
    let mut previous = 0u8;
    let mut p = 0;
    let mut in_word = false;
    while p < input.len() {
        let b = input[p];
        if b == b' ' {
            if in_word {
                if word.is_empty() {
                    return Ok(out);
                }
                out.push(std::mem::take(&mut word));
            }
            previous = 0;
            in_word = false;
            p += 1;
        } else if b == b'(' {
            if in_word && previous.is_ascii_alphabetic() {
                if word.is_empty() {
                    return Ok(out);
                }
                out.push(std::mem::take(&mut word));
            }
            let start = p;
            let mut depth = 0usize;
            loop {
                match input.get(p) {
                    Some(b'(') => depth += 1,
                    Some(b')') => depth -= 1,
                    None => return Err(Error::new(start, "未闭合圆括号")),
                    _ => (),
                }
                p += 1;
                if depth == 0 {
                    break;
                }
            }
            word.extend(if input.get(start + 1) == Some(&b':') {
                &input[start + 2..p - 1]
            } else {
                &input[start..p]
            });
            in_word = true;
        } else {
            if b == b')' {
                return Err(Error::new(p, "孤立右括号"));
            }
            word.push(b);
            previous = b;
            in_word = true;
            p += 1;
        }
        if out.len() > 4096 {
            return Err(Error::new(p, "参数数量超过上限"));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    Ok(out)
}

pub fn condition(input: &[u8]) -> Result<Option<(&[u8], &[u8])>> {
    if !input.starts_with(b"if")
        || !input
            .get(2)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'(')
    {
        return Ok(None);
    }
    let start = input[2..]
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| i + 2)
        .unwrap_or(input.len());
    if input.get(start) != Some(&b'(') {
        return Err(Error::new(start, "if 需要圆括号条件"));
    }
    let mut depth = 0;
    for (p, &b) in input.iter().enumerate().skip(start) {
        if b == b'(' {
            depth += 1;
        }
        if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok(Some((
                    &input[start + 1..p],
                    input[p + 1..].trim_ascii_start(),
                )));
            }
        }
    }
    Err(Error::new(start, "if 条件未闭合"))
}
