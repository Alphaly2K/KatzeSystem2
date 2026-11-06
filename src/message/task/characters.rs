use std::collections::BTreeSet;

use super::{BreakCharacters, TextEncoding};
use crate::{Error, Result, text};

pub(super) struct CharacterSets {
    encoding: TextEncoding,
    sets: [BTreeSet<u16>; 5],
}

impl CharacterSets {
    pub fn new(encoding: TextEncoding) -> Self {
        let defaults = BreakCharacters::for_encoding(encoding);
        let sets = [
            defaults.no_start,
            defaults.no_end,
            defaults.repeated,
            defaults.indent_open,
            defaults.indent_keep,
        ]
        .map(|characters| {
            characters
                .chars()
                .map(|character| match encoding {
                    TextEncoding::Unicode => character as u16,
                    TextEncoding::Cp932 => {
                        let mut buffer = [0; 4];
                        let (bytes, _, invalid) =
                            encoding_rs::SHIFT_JIS.encode(character.encode_utf8(&mut buffer));
                        assert!(!invalid);
                        bytes
                            .iter()
                            .fold(0, |code, &byte| (code << 8) | u16::from(byte))
                    }
                })
                .collect()
        });
        Self { encoding, sets }
    }

    pub fn add(&mut self, index: u32, bytes: &[u8]) -> Result<()> {
        if self.encoding == TextEncoding::Unicode {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| Error::new(0, "消息字符集合需要有效 UTF-8"))?;
            if text.encode_utf16().count() >= 2048 {
                return Err(Error::new(0, "消息字符集合超过转换缓冲区容量"));
            }
            if let Some(set) = self.sets.get_mut(index as usize) {
                set.extend(text.encode_utf16());
            }
        } else if let Some(set) = self.sets.get_mut(index as usize) {
            let mut at = 0;
            while let Some(&first) = bytes.get(at) {
                let wide = lead(first);
                let next = bytes.get(at + 1).copied().unwrap_or(0);
                set.insert(if wide {
                    pair(first, next)
                } else {
                    u16::from(first)
                });
                at += if wide { 2 } else { 1 };
            }
        }
        Ok(())
    }

    pub fn remove(&mut self, index: u32, bytes: &[u8]) {
        let Some(set) = self.sets.get_mut(index as usize) else {
            return;
        };
        let mut at = 0;
        while let Some(&first) = bytes.get(at) {
            // 两种对象的移除入口均按旧字节规则拼编号，单字节也读取后一字节。
            set.remove(&pair(first, bytes.get(at + 1).copied().unwrap_or(0)));
            at += if lead(first) { 2 } else { 1 };
        }
    }

    pub fn clear(&mut self, index: u32) {
        if let Some(set) = self.sets.get_mut(index as usize) {
            set.clear();
        }
    }

    pub fn layout(&self) -> BreakCharacters {
        let [no_start, no_end, repeated, indent_open, indent_keep] =
            self.sets.each_ref().map(|set| {
                set.iter()
                    .filter_map(|&code| match self.encoding {
                        TextEncoding::Unicode => char::from_u32(u32::from(code)),
                        TextEncoding::Cp932 => {
                            let bytes = code.to_be_bytes();
                            text::cp932(if code > 255 { &bytes } else { &bytes[1..] })
                                .ok()
                                .and_then(|text| text.chars().next())
                        }
                    })
                    .collect()
            });
        BreakCharacters {
            no_start,
            no_end,
            repeated,
            indent_open,
            indent_keep,
        }
    }
}

fn lead(byte: u8) -> bool {
    matches!(byte, 0x81..=0x9f | 0xe0..=0xfc)
}

fn pair(first: u8, next: u8) -> u16 {
    if next == 0 {
        first as i8 as i16 as u16
    } else {
        u16::from_be_bytes([first, next])
    }
}
