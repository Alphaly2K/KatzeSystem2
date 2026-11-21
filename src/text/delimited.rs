use crate::{Error, Result};

pub(crate) fn field(input: &mut &[u8], delimiter: u8) -> Result<Vec<u8>> {
    let quoted = input.starts_with(b"\"");
    if quoted {
        *input = &input[1..];
    }
    let mut value = Vec::new();
    while let Some(&byte) = input.first() {
        match byte {
            b'\r' | b'\n' if !quoted => break,
            byte if byte == delimiter && !quoted => break,
            b'\r' | b'\n' => {
                let pair = if byte == b'\r' { b"\r\n" } else { b"\n\r" };
                *input = &input[if input.starts_with(pair) { 2 } else { 1 }..];
            }
            b'"' if !input.starts_with(b"\"\"") => {
                *input = &input[1..];
                break;
            }
            b'"' => {
                value.push(byte);
                *input = &input[2..];
            }
            _ => {
                let width = char_width(input);
                value.extend_from_slice(&input[..width]);
                *input = &input[width..];
            }
        }
        if value.len() > 255 {
            return Err(Error::new(0, "分隔表字段超过缓冲区上限"));
        }
    }
    while input
        .first()
        .is_some_and(|b| *b != delimiter && !matches!(b, b'\r' | b'\n'))
    {
        *input = &input[char_width(input)..];
    }
    if input.first() == Some(&delimiter) {
        *input = &input[1..];
    }
    Ok(value)
}

pub(crate) fn char_width(bytes: &[u8]) -> usize {
    usize::from(
        matches!(bytes[0], 0x81..=0x9f | 0xe0..=0xef)
            && bytes
                .get(1)
                .is_some_and(|b| matches!(b, 0x40..=0x7e | 0x80..=0xfc)),
    ) + 1
}
