use super::Scene;
use crate::{Error, Result, expression::State};

const CAPACITY: usize = 2560;

impl Scene {
    pub(super) fn string_command(
        &mut self,
        args: &[Vec<u8>],
        offset: usize,
        numbers: &mut State,
    ) -> Result<bool> {
        if args.first().is_none_or(|name| name != b"strvar") {
            return Ok(false);
        }
        let Some(first) = args.get(1) else {
            return Ok(true);
        };
        let (index, mut value) = if first.eq_ignore_ascii_case(b"get") {
            let Some(index) = args.get(2) else {
                return Ok(true);
            };
            let index = self.interpreter.eval(index, numbers)? as u32;
            let Some(source) = args.get(3) else {
                return Ok(true);
            };
            let source = self.interpreter.eval(source, numbers)? as u32;
            (
                index,
                numbers.strings.get(&source).cloned().unwrap_or_default(),
            )
        } else {
            let index = self.interpreter.eval(first, numbers)? as u32;
            let format = args.get(2).map_or(&[][..], Vec::as_slice);
            let mut values = Vec::new();
            for input in args.iter().skip(3).take(4) {
                values.push(self.interpreter.eval(input, numbers)? as u32);
            }
            (
                index,
                format_integers(format, &values)
                    .map_err(|error| Error::new(offset + error.offset, error.message))?,
            )
        };
        let end = value
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(value.len());
        value.truncate(end);
        numbers.strings.insert(index, value);
        Ok(true)
    }
}

fn append(output: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    if output.len() + value.len() >= CAPACITY {
        return Err(Error::new(0, "FES 字符串输出超过容量"));
    }
    output.extend(value);
    Ok(())
}

fn padding(output: &mut Vec<u8>, byte: u8, count: usize) -> Result<()> {
    if output.len() + count >= CAPACITY {
        return Err(Error::new(0, "FES 字符串输出超过容量"));
    }
    output.resize(output.len() + count, byte);
    Ok(())
}

fn field(source: &[u8], cursor: &mut usize) -> Result<usize> {
    let mut value = 0;
    while let Some(&byte) = source.get(*cursor).filter(|byte| byte.is_ascii_digit()) {
        value = value * 10 + usize::from(byte - b'0');
        if value >= CAPACITY {
            return Err(Error::new(*cursor, "FES 格式字段超过容量"));
        }
        *cursor += 1;
    }
    Ok(value)
}

fn format_integers(format: &[u8], values: &[u32]) -> Result<Vec<u8>> {
    let end = format
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(format.len());
    let format = &format[..end];
    let mut output = Vec::new();
    if values.is_empty() {
        append(&mut output, format)?;
        return Ok(output);
    }
    let mut arguments = values.iter();
    let mut argument = || {
        arguments
            .next()
            .copied()
            .ok_or_else(|| Error::new(0, "FES 格式字符串缺少整数参数"))
    };
    let mut cursor = 0;
    while cursor < format.len() {
        if format[cursor] != b'%' {
            append(&mut output, &format[cursor..cursor + 1])?;
            cursor += 1;
            continue;
        }
        cursor += 1;
        if format.get(cursor) == Some(&b'%') {
            append(&mut output, b"%")?;
            cursor += 1;
            continue;
        }
        let (mut left, mut plus, mut space, mut alternate, mut zero) =
            (false, false, false, false, false);
        loop {
            match format.get(cursor) {
                Some(b'-') => left = true,
                Some(b'+') => plus = true,
                Some(b' ') => space = true,
                Some(b'#') => alternate = true,
                Some(b'0') => zero = true,
                _ => break,
            }
            cursor += 1;
        }
        let width = if format.get(cursor) == Some(&b'*') {
            cursor += 1;
            let width = argument()? as i32;
            left |= width < 0;
            width.unsigned_abs() as usize
        } else {
            field(format, &mut cursor)?
        };
        if width >= CAPACITY {
            return Err(Error::new(cursor, "FES 格式字段超过容量"));
        }
        let precision = if format.get(cursor) == Some(&b'.') {
            cursor += 1;
            if format.get(cursor) == Some(&b'*') {
                cursor += 1;
                let precision = argument()? as i32;
                (precision >= 0).then_some(precision as usize)
            } else {
                Some(field(format, &mut cursor)?)
            }
        } else {
            None
        };
        if precision.is_some_and(|precision| precision >= CAPACITY) {
            return Err(Error::new(cursor, "FES 格式字段超过容量"));
        }
        let modifier_start = cursor;
        let bits = if format[cursor..].starts_with(b"I64") {
            cursor += 3;
            64
        } else if format[cursor..].starts_with(b"I32") {
            cursor += 3;
            32
        } else if format[cursor..].starts_with(b"hh") {
            cursor += 2;
            // 基线 CRT 的重复 h 仍按十六位处理。
            16
        } else if format.get(cursor) == Some(&b'h') {
            cursor += 1;
            16
        } else if format[cursor..].starts_with(b"ll") {
            cursor += 2;
            64
        } else {
            if format.get(cursor) == Some(&b'l') {
                cursor += 1;
            }
            32
        };
        let conversion = *format
            .get(cursor)
            .ok_or_else(|| Error::new(cursor, "FES 格式转换未结束"))?;
        cursor += 1;
        if !matches!(conversion, b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c') {
            return Err(Error::new(cursor - 1, "FES 此格式转换尚未接入"));
        }
        if conversion == b'c' && cursor - 1 != modifier_start {
            return Err(Error::new(cursor - 1, "FES 带长度修饰的字符格式尚未接入"));
        }
        let low = argument()?;
        let raw = match bits {
            16 => u64::from(low as u16),
            64 => u64::from(low) | (u64::from(argument()?) << 32),
            _ => u64::from(low),
        };
        let mut prefix = Vec::new();
        let digits = if conversion == b'c' {
            vec![low as u8]
        } else {
            let signed = matches!(conversion, b'd' | b'i');
            let magnitude = if signed {
                let value = match bits {
                    16 => raw as i16 as i64,
                    32 => raw as i32 as i64,
                    _ => raw as i64,
                };
                if value < 0 {
                    prefix.push(b'-');
                } else if plus {
                    prefix.push(b'+');
                } else if space {
                    prefix.push(b' ');
                }
                value.unsigned_abs()
            } else {
                raw
            };
            let digits = if magnitude == 0 && precision == Some(0) {
                Vec::new()
            } else {
                match conversion {
                    b'x' => format!("{magnitude:x}"),
                    b'X' => format!("{magnitude:X}"),
                    b'o' => format!("{magnitude:o}"),
                    _ => magnitude.to_string(),
                }
                .into_bytes()
            };
            let mut padded = vec![b'0'; precision.unwrap_or(0).saturating_sub(digits.len())];
            padded.extend(digits);
            if alternate {
                match conversion {
                    b'x' if raw != 0 => prefix.extend(b"0x"),
                    b'X' if raw != 0 => prefix.extend(b"0X"),
                    b'o' if raw != 0 && padded.first() != Some(&b'0') => prefix.push(b'0'),
                    _ => (),
                }
            }
            zero &= precision.is_none();
            padded
        };
        let count = width.saturating_sub(prefix.len() + digits.len());
        if !left && !zero {
            padding(&mut output, b' ', count)?;
        }
        append(&mut output, &prefix)?;
        if !left && zero {
            padding(&mut output, b'0', count)?;
        }
        append(&mut output, &digits)?;
        if left {
            padding(&mut output, b' ', count)?;
        }
    }
    Ok(output)
}
