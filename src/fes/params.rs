use crate::{Error, Result, read};

/// 按 FES 参数入口读取字节词项；双引号后的隐藏词项不在返回列表中。
pub fn parse(input: &[u8]) -> Result<Vec<Vec<u8>>> {
    if input.len() > read::MAX_BYTES {
        return Err(Error::new(0, "FES 参数文本过长"));
    }
    let mut table = Vec::new();
    let mut p = 0;
    let mut cursor = 0;
    let mut previous = 0u8;
    let mut separated = true;
    while p < input.len() && input[p] != 0 {
        let b = input[p];
        if matches!(b, b'(' | b'"') {
            if previous.is_ascii_alphabetic() && !separated {
                put(&mut table, cursor, &[0])?;
                cursor += 1;
            }
            let start = p;
            p += 1;
            if b == b'(' {
                let mut depth = 1;
                while depth > 0 && input.get(p).is_some_and(|&b| b != 0) {
                    match input.get(p) {
                        Some(b'(') => depth += 1,
                        Some(b')') => depth -= 1,
                        _ => (),
                    }
                    p += 1;
                }
                put(&mut table, cursor, &input[start..p])?;
                put(&mut table, cursor + p - start, &[0, 0])?;
            } else {
                while input.get(p).is_some_and(|&b| b != b'"' && b != 0) {
                    p += 1;
                }
                put(&mut table, cursor, &input[start + 1..p])?;
                put(&mut table, cursor + p - start - 1, &[0, 0])?;
                p += usize::from(input.get(p) == Some(&b'"'));
            }
            let consumed = p - start;
            if input.get(start + 1) == Some(&b':') {
                if consumed < 3 {
                    return Err(Error::new(start, "FES 文本参数无效"));
                }
                let length = consumed - 3;
                table.copy_within(cursor + 2..cursor + 2 + length, cursor);
                put(&mut table, cursor + length, &[0])?;
                cursor += length;
            } else {
                cursor += consumed;
            }
            separated = false;
        } else if b == b' ' {
            if !separated {
                put(&mut table, cursor, &[0])?;
                cursor += 1;
            }
            previous = 0;
            separated = true;
            p += 1;
        } else {
            put(&mut table, cursor, &[b])?;
            cursor += 1;
            previous = b;
            separated = false;
            p += 1;
        }
    }
    put(&mut table, cursor, &[0, 0])?;
    let words: Vec<_> = table
        .split(|&b| b == 0)
        .take_while(|word| !word.is_empty())
        .take(read::MAX_ITEMS + 1)
        .map(<[u8]>::to_vec)
        .collect();
    if words.len() > read::MAX_ITEMS {
        return Err(Error::new(0, "FES 参数数量超过上限"));
    }
    Ok(words)
}

fn put(table: &mut Vec<u8>, at: usize, bytes: &[u8]) -> Result<()> {
    let end = at
        .checked_add(bytes.len())
        .filter(|&n| n <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(at, "FES 参数表过长"))?;
    if end > table.len() {
        table.resize(end, 0);
    }
    table[at..end].copy_from_slice(bytes);
    Ok(())
}
