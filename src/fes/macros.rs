use crate::{Error, Result, read};
use std::collections::BTreeMap;

pub(super) type Macros = BTreeMap<Vec<u8>, Vec<u8>>;

pub(super) fn declaration(line: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let split = line
        .iter()
        .position(u8::is_ascii_whitespace)
        .ok_or_else(|| Error::new(0, "宏定义缺少值"))?;
    let name = &line[..split];
    let tail = line[split..].trim_ascii_start();
    let value = if tail.first() == Some(&b'"') {
        let end = tail[1..]
            .iter()
            .position(|&b| b == b'"')
            .ok_or_else(|| Error::new(split, "宏值引号未闭合"))?
            + 1;
        &tail[1..end]
    } else if tail.first() == Some(&b'(') {
        let mut depth = 0;
        let mut end = None;
        for (i, &b) in tail.iter().enumerate() {
            if b == b'(' {
                depth += 1;
            }
            if b == b')' {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
        }
        let end = end.ok_or_else(|| Error::new(split, "宏值圆括号未闭合"))?;
        if tail.get(1) == Some(&b':') {
            &tail[2..end - 1]
        } else {
            &tail[..end]
        }
    } else {
        tail.split(u8::is_ascii_whitespace).next().unwrap_or(&[])
    };
    if name.is_empty() || value.is_empty() {
        return Err(Error::new(0, "宏名和值不能为空"));
    }
    Ok((name.to_vec(), value.to_vec()))
}

pub(super) fn expand(input: &[u8], macros: &Macros) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut p = 0;
    while p < input.len() {
        let start = p;
        let b = input[p];
        let replacement = if matches!(b, b'"' | b'\'') {
            p += 1;
            while p < input.len() {
                if (matches!(input[p], 0x81..=0x9f | 0xe0..=0xfc) && p + 1 < input.len())
                    || (input[p] == b'\\'
                        && input
                            .get(p + 1)
                            .is_some_and(|&next| next == b || (b == b'\'' && next == b'\\')))
                {
                    p += 2;
                } else if input[p] == b {
                    p += 1;
                    break;
                } else {
                    p += 1;
                }
            }
            None
        } else if b.is_ascii_digit() {
            if input[p..].starts_with(b"0x") || input[p..].starts_with(b"0X") {
                p += 2;
                while input.get(p).is_some_and(u8::is_ascii_hexdigit) {
                    p += 1;
                }
            } else {
                while input.get(p).is_some_and(u8::is_ascii_digit) {
                    p += 1;
                }
                if input.get(p) == Some(&b'.') {
                    p += 1;
                    while input.get(p).is_some_and(u8::is_ascii_digit) {
                        p += 1;
                    }
                }
            }
            None
        } else if b.is_ascii_alphabetic() || b == b'_' {
            p += 1;
            while input
                .get(p)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                p += 1;
            }
            macros.get(&input[start..p])
        } else {
            p += 1;
            // CP932 的第二字节可落入 ASCII 字母范围。
            if matches!(b, 0x81..=0x9f | 0xe0..=0xfc) && p < input.len() {
                p += 1;
            }
            None
        };
        let bytes = replacement.map_or(&input[start..p], Vec::as_slice);
        if output
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > read::MAX_BYTES)
        {
            return Err(Error::new(start, "FES 宏展开超过上限"));
        }
        output.extend(bytes);
    }
    Ok(output)
}
