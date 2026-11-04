use super::Settings;
use crate::{Error, Result};

pub(super) fn preprocess(input: &[u8], escape: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < input.len() {
        let remaining = &input[at..];
        if remaining[0] == b'\\' {
            let code = remaining.get(1).copied().unwrap_or(0);
            if escape {
                if code == b'@' || matches!(code, b'r' | b'R') {
                    at += if matches!(code, b'r' | b'R')
                        && remaining.get(2) == Some(&b'\\')
                        && matches!(remaining.get(3), Some(b'n' | b'N'))
                    {
                        4
                    } else {
                        2
                    };
                    continue;
                }
            } else {
                let length = match code.to_ascii_lowercase() {
                    b'@' | b'n' => 2,
                    b'c' => {
                        let prefix = if matches!(remaining.get(2), Some(b'r' | b'R' | b's' | b'S'))
                        {
                            3
                        } else {
                            2
                        };
                        prefix + number_length(&remaining[prefix.min(remaining.len())..])
                    }
                    b'f' => {
                        let sub = remaining.get(2).copied().unwrap_or(0).to_ascii_lowercase();
                        match sub {
                            b'+' | b'-' | b'r' => {
                                3 + number_length(remaining.get(3..).unwrap_or_default())
                            }
                            b'l' | b's' => {
                                let extra = matches!(
                                    (
                                        sub,
                                        remaining.get(3).copied().unwrap_or(0).to_ascii_lowercase()
                                    ),
                                    (b'l', b'l') | (b's', b's')
                                );
                                let length = 3 + usize::from(extra);
                                length + usize::from(remaining.get(length) == Some(&b';'))
                            }
                            b'n' => 3 + usize::from(remaining.get(4) == Some(&b';')),
                            _ => 2 + number_length(remaining.get(2..).unwrap_or_default()),
                        }
                    }
                    b'p' => {
                        2 + usize::from(matches!(
                            remaining.get(2),
                            Some(b'c' | b'C' | b'l' | b'L' | b'r' | b'R')
                        ))
                    }
                    b'w' | b'x' | b'y' => 2 + number_length(remaining.get(2..).unwrap_or_default()),
                    b'[' | b'\\' => {
                        out.extend_from_slice(&remaining[..remaining.len().min(2)]);
                        at += 2;
                        continue;
                    }
                    _ => 1,
                };
                at = at.saturating_add(length);
                continue;
            }
        }
        let length = if is_pair(remaining, 0) {
            remaining.len().min(2)
        } else {
            1
        };
        out.extend_from_slice(&remaining[..length]);
        at += length;
    }
    out
}

fn number_length(input: &[u8]) -> usize {
    let sign = usize::from(matches!(input.first(), Some(b'+' | b'-')));
    if !input.get(sign).is_some_and(u8::is_ascii_digit) {
        return 0;
    }
    let mut at = sign;
    if matches!(input.get(at..at + 2), Some(b"0x" | b"0X")) {
        at += 2;
        while input.get(at).is_some_and(u8::is_ascii_hexdigit) {
            at += 1;
        }
    } else {
        while input.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    at + usize::from(input.get(at) == Some(&b';'))
}

fn is_pair(input: &[u8], at: usize) -> bool {
    matches!(input.get(at), Some(0x81..=0x9f | 0xe0..=0xef))
        && matches!(input.get(at + 1), Some(0x40..=0x7e | 0x80..=0xfc))
}

fn token(input: &[u8], at: usize) -> (u16, usize) {
    let Some(&first) = input.get(at) else {
        return (0, 0);
    };
    if is_pair(input, at) {
        return (
            ((first as u16) << 8) | input.get(at + 1).copied().unwrap_or(0) as u16,
            2,
        );
    }
    if first == b'\\' && matches!(input.get(at + 1), Some(b'[' | b'\\' | b'n')) {
        return ((0x5c00 | input[at + 1] as u16), 2);
    }
    (first as i8 as i16 as u16, 1)
}

fn indentation(width: usize) -> Vec<u8> {
    let mut text = Vec::with_capacity(width);
    for _ in 0..width / 2 {
        text.extend_from_slice(&[0x81, 0x40]);
    }
    if !width.is_multiple_of(2) {
        text.push(b' ');
    }
    text
}

pub(super) fn split(settings: &Settings, text: &[u8], prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
    let columns = settings.columns as i32;
    let profile = settings.profile as i32;
    let initial_indent = settings.name_indent as i32;
    if columns <= 0 || columns > 1024 || initial_indent < 0 || initial_indent >= columns {
        return Err(Error::new(0, "回看日志分行配置超出有效范围"));
    }
    let (first, first_width) = token(text, 0);
    let hanging = if matches!(first, 0x8169 | 0x816d | 0x816f | 0x8175 | 0x8177) {
        first_width
    } else {
        0
    };
    let mut indent = initial_indent;
    let mut prefix = if settings.display_name != 0 && settings.name_line == 0 {
        prefix.to_vec()
    } else {
        Vec::new()
    };
    if !prefix.is_empty() {
        indent -= prefix.len() as i32;
    }
    let mut at = 0;
    let mut rows = Vec::new();
    while at < text.len() {
        let start = at;
        let mut line = prefix;
        if indent > 0 {
            line.extend_from_slice(&indentation(indent as usize));
        }
        let mut width = line.len() as i32;
        while at < text.len() && width < columns {
            let (word, length) = token(text, at);
            if word == b'[' as u16 {
                let mut end = at + 1;
                let mut base_width = 0;
                let mut reading = false;
                while end < text.len() {
                    let (word, length) = token(text, end);
                    end += length;
                    if word == b']' as u16 {
                        break;
                    }
                    if word == b'/' as u16 {
                        reading = true;
                    } else if !reading {
                        base_width += length as i32;
                    }
                }
                if width + base_width > columns.saturating_add(profile) {
                    break;
                }
                width += base_width;
                at = end.min(text.len());
            } else {
                if width + length as i32 >= columns {
                    break;
                }
                width += length as i32;
                at += length;
            }
        }
        if at == start {
            return Err(Error::new(0, "回看日志分行无法前进"));
        }
        line.extend_from_slice(&text[start..at.min(text.len())]);
        rows.push(line);
        indent = initial_indent + hanging as i32;
        prefix = Vec::new();
    }
    Ok(rows)
}
