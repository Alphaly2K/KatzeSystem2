use crate::{Error, Result, read};

mod channel;
mod config;
mod control;
mod groups;
mod pcm;
mod playback;
mod task;
pub use channel::SoundChannel;
pub use config::{SoundConfig, SoundPart};
pub use groups::{SoundGroup, SoundGroups};
pub use pcm::{Pcm, decode_vorbis};
pub use task::{SoundFactory, SoundObject, SoundState};

/// 声音任务登记的原始循环范围；计数尚未转换为播放设备的单位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopPoint {
    pub name: Vec<u8>,
    pub start: i64,
    pub end: i64,
}

/// 读取普通 400 的循环点表，名称尚未经过声音类别补全。
pub fn parse_loop_table(bytes: &[u8]) -> Result<Vec<LoopPoint>> {
    if bytes.len() > read::MAX_BYTES {
        return Err(Error::new(0, "声音循环点表超过读取上限"));
    }
    let bytes = terminated(bytes);
    let mut text = Vec::with_capacity(bytes.len());
    let mut p = 0;
    let mut block = false;
    while p < bytes.len() {
        if block {
            if bytes[p..].starts_with(b"*/") {
                block = false;
                p += 2;
            } else {
                if matches!(bytes[p], b'\r' | b'\n') {
                    text.push(bytes[p]);
                }
                p += 1;
            }
        } else if bytes[p..].starts_with(b"/*") {
            block = true;
            p += 2;
        } else if bytes[p..].starts_with(b"//") {
            while p < bytes.len() && !matches!(bytes[p], b'\r' | b'\n') {
                p += 1;
            }
        } else {
            if bytes[p] != b'"' {
                text.push(bytes[p]);
            }
            p += 1;
        }
    }
    let mut points = Vec::new();
    for line in text.split(|&b| matches!(b, b'\r' | b'\n')) {
        let mut words = line
            .split(|&b| matches!(b, b' ' | b'\t'))
            .filter(|word| !word.is_empty());
        let Some(name) = words.next() else { continue };
        if points.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "声音循环点数量超过存储上限"));
        }
        points.push(LoopPoint {
            name: name.to_vec(),
            start: words.next().map_or(0, loop_integer),
            end: words.next().map_or(-1, loop_integer),
        });
    }
    Ok(points)
}

fn loop_integer(mut bytes: &[u8]) -> i64 {
    let negative = bytes.first() == Some(&b'-');
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        bytes = &bytes[1..];
    }
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return 0;
    }
    let hex = bytes.starts_with(b"0x") || bytes.starts_with(b"0X");
    if hex {
        bytes = &bytes[2..];
    }
    let mut value = 0u64;
    for &byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' if hex => byte - b'a' + 10,
            b'A'..=b'F' if hex => byte - b'A' + 10,
            b'.' if !hex => return 0,
            _ => break,
        };
        value = value
            .wrapping_mul(if hex { 16 } else { 10 })
            .wrapping_add(digit as u64);
    }
    if negative {
        value.wrapping_neg() as i64
    } else {
        value as i64
    }
}

/// 声音命令 199 的名称与循环范围，按首次登记顺序保存。
#[derive(Clone, Debug, Default)]
pub struct LoopPoints {
    entries: Vec<LoopPoint>,
}

impl LoopPoints {
    pub fn entries(&self) -> &[LoopPoint] {
        &self.entries
    }

    pub fn find(&self, name: &[u8]) -> Option<&LoopPoint> {
        let name = terminated(name);
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// 结束值为 -1，或起点严格小于终点时登记；其他范围保持原状态。
    pub fn register(&mut self, name: &[u8], start: i64, end: i64) -> Result<()> {
        if end != -1 && start >= end {
            return Ok(());
        }
        let name = terminated(name);
        if name.len() >= 32 {
            return Err(Error::new(0, "声音循环点名称超过配置容量"));
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
        {
            // 基线重复登记只复制起点，保留首次登记的终点及名称。
            entry.start = start;
        } else {
            if self.entries.len() >= read::MAX_ITEMS {
                return Err(Error::new(0, "声音循环点数量超过存储上限"));
            }
            self.entries.push(LoopPoint {
                name: name.to_vec(),
                start,
                end,
            });
        }
        Ok(())
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
