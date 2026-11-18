use crate::{Error, Result, read};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub(super) struct Settings {
    pub(super) count: u32,
    pub(super) quota: u32,
    pub(super) background: Vec<u8>,
    pub(super) rival: u32,
    pub(super) rival_type: u32,
    pub(super) rival_image: Vec<u8>,
    pub(super) rival_position: [u32; 3],
    pub(super) members: BTreeSet<u32>,
    pub(super) partners: BTreeSet<u32>,
    pub(super) music: u32,
    pub(super) disabled: u32,
    pub(super) experience: u32,
    pub(super) results: [u32; 4],
    pub(super) genres: [Vec<i32>; 3],
}

pub(super) fn parse(bytes: &[u8]) -> Result<BTreeMap<Vec<u8>, Settings>> {
    let mut lines = lines(super::take_before_zero(bytes))?;
    let mut settings = BTreeMap::new();
    let mut quota = 0;
    while let Some(header) = lines.next() {
        let Some(name) = header.strip_prefix(b"#") else {
            break;
        };
        if name.len() >= 256 || settings.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "问答设置名称或条目数量超过上限"));
        }
        let mut entry = Settings {
            rival: u32::MAX,
            quota,
            ..Settings::default()
        };
        while let Some(line) = lines.peek() {
            if !entry.field(line)? {
                break;
            }
            lines.next();
        }
        if lines.peek().is_some_and(|line| line == b"$GEN") {
            lines.next();
            while let Some(line) = lines.peek() {
                {
                    let mut words = words(line);
                    match words.next() {
                        Some(b"GENRE_A") => genres(&mut entry.genres[0], words)?,
                        Some(b"GENRE_B") => genres(&mut entry.genres[1], words)?,
                        Some(b"GENRE_C") => genres(&mut entry.genres[2], words)?,
                        Some(b"DISABLE") => {}
                        _ => break,
                    }
                }
                lines.next();
            }
        }
        quota = entry.quota;
        settings.entry(name.to_vec()).or_insert(entry);
    }
    Ok(settings)
}

impl Settings {
    fn field(&mut self, line: &[u8]) -> Result<bool> {
        let mut fields = words(line);
        let name = fields.next();
        let value = fields.next();
        let number = value.map(integer);
        match name {
            Some(b"CNT") => self.count = number.unwrap_or_default(),
            Some(b"NORMA") => set(&mut self.quota, number),
            Some(b"BG") => copy_name(&mut self.background, value)?,
            Some(b"RIVAL") => set(&mut self.rival, number),
            Some(b"RTYPE") => set(&mut self.rival_type, number),
            Some(b"RIMG") => copy_name(&mut self.rival_image, value)?,
            Some(b"RPOS") => {
                if let Some(x) = number {
                    self.rival_position[..2].copy_from_slice(&[1, x]);
                    set(&mut self.rival_position[2], fields.next().map(integer));
                }
            }
            Some(b"MEMBER") => self
                .members
                .extend(value.into_iter().chain(fields).map(integer)),
            Some(b"PARTNER") => self
                .partners
                .extend(value.into_iter().chain(fields).map(integer)),
            Some(b"BGM") => set(&mut self.music, number),
            Some(b"EXP") => set(&mut self.experience, number),
            Some(b"PWIN") => set(&mut self.results[0], number),
            Some(b"PLOSE") => set(&mut self.results[1], number),
            Some(b"RWIN") => set(&mut self.results[2], number),
            Some(b"RLOSE") => set(&mut self.results[3], number),
            Some(b"DISABLE") => {
                for bit in value.into_iter().chain(fields).map(integer) {
                    self.disabled |= 1u32.rotate_left(bit.wrapping_sub(1));
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

fn words(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|&byte| byte == b' ')
        .filter(|word| !word.is_empty())
}

fn lines(bytes: &[u8]) -> Result<std::iter::Peekable<impl Iterator<Item = Vec<u8>>>> {
    if bytes.len() > read::MAX_BYTES {
        return Err(Error::new(0, "问答设置文件超过大小上限"));
    }
    let mut block = false;
    let mut lines = Vec::new();
    for line in bytes.split(|&b| matches!(b, b'\r' | b'\n')) {
        let mut text = Vec::new();
        let mut rest = line;
        while let Some((&byte, tail)) = rest.split_first() {
            if block {
                if let Some(next) = rest.strip_prefix(b"*/") {
                    block = false;
                    rest = next;
                } else {
                    rest = tail;
                }
            } else if let Some(next) = rest.strip_prefix(b"/*") {
                block = true;
                rest = next;
            } else if rest.starts_with(b"//") {
                break;
            } else {
                if matches!(byte, b' ' | b'\t') {
                    if !text.is_empty() && text.last() != Some(&b' ') {
                        text.push(b' ');
                    }
                } else {
                    text.push(byte);
                }
                rest = tail;
            }
        }
        if text.last() == Some(&b' ') {
            text.pop();
        }
        if !text.is_empty() {
            if lines.len() >= read::MAX_ITEMS || text.len() >= 512 {
                return Err(Error::new(0, "问答设置行超过固定缓冲区上限"));
            }
            lines.push(text);
        }
    }
    Ok(lines.into_iter().peekable())
}

fn set(destination: &mut u32, value: Option<u32>) {
    if let Some(value) = value {
        *destination = value;
    }
}

fn copy_name(destination: &mut Vec<u8>, value: Option<&[u8]>) -> Result<()> {
    if let Some(value) = value {
        if value.len() >= 64 {
            return Err(Error::new(0, "问答设置的图像名称超过固定字段容量"));
        }
        *destination = value.to_vec();
    }
    Ok(())
}

fn integer(word: &[u8]) -> u32 {
    parse_integer(word).0.unwrap_or(0)
}

fn parse_integer(mut word: &[u8]) -> (Option<u32>, &[u8]) {
    let negative = word.first() == Some(&b'-');
    if matches!(word.first(), Some(b'+' | b'-')) {
        word = &word[1..];
    }
    if !word.first().is_some_and(u8::is_ascii_digit) {
        return (None, word);
    }
    let radix = if let Some(hex) = word
        .strip_prefix(b"0x")
        .or_else(|| word.strip_prefix(b"0X"))
    {
        word = hex;
        16
    } else {
        10
    };
    let mut value = 0u32;
    while let Some(digit) = word
        .first()
        .and_then(|&byte| (byte as char).to_digit(radix))
    {
        value = value.wrapping_mul(radix).wrapping_add(digit);
        word = &word[1..];
    }
    let value = if radix == 10 && word.first() == Some(&b'.') {
        None
    } else {
        Some(if negative {
            value.wrapping_neg()
        } else {
            value
        })
    };
    (value, word)
}

pub(super) const DYNAMIC_GENRE: i32 = -10;

fn genres<'a>(list: &mut Vec<i32>, words: impl Iterator<Item = &'a [u8]>) -> Result<()> {
    for word in words {
        let (first, rest) = parse_integer(word);
        let first = if word.starts_with(b"@") {
            DYNAMIC_GENRE
        } else {
            first.unwrap_or(u32::MAX) as i32
        };
        let last = if word.starts_with(b"@") {
            first
        } else {
            rest.strip_prefix(b"-")
                .and_then(|last| parse_integer(last).0)
                .map(|last| last as i32)
                .filter(|&last| last != -1)
                .unwrap_or(first)
        };
        if first <= last {
            let count = i64::from(last) - i64::from(first) + 1;
            if count > (read::MAX_ITEMS - list.len()) as i64 {
                return Err(Error::new(0, "问答分组选项超过存储上限"));
            }
            list.extend(first..=last);
        }
    }
    Ok(())
}
