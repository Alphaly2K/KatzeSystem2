use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// UTF-8 字符串表；键和值均保留完整 UTF-16，重复键采用首项。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringTable(BTreeMap<Vec<u16>, Vec<u16>>);

impl StringTable {
    pub fn parse(input: &[u8]) -> Result<Self> {
        if input.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字符串表超过读取上限"));
        }
        let input = super::terminated(input);
        let input = input.strip_prefix(b"\xef\xbb\xbf").unwrap_or(input);
        let bytes = remove_comments(input);
        let mut table = BTreeMap::new();
        let mut key = None;
        let mut count = 0;
        for line in bytes.split(|&byte| matches!(byte, b'\r' | b'\n')) {
            let line = line.strip_prefix(b" ").unwrap_or(line);
            let line = line.strip_suffix(b" ").unwrap_or(line);
            if line.is_empty() || line.starts_with(b" ") {
                continue;
            }
            count += 1;
            if count > read::MAX_ITEMS {
                return Err(Error::new(0, "字符串表行数超过上限"));
            }
            if let Some(key) = key.take() {
                if line != b"$nr$" {
                    let value = if line == b"$empty$" { &[] } else { line };
                    table.entry(key).or_insert_with(|| decode_utf8(value));
                }
            } else {
                key = Some(decode_utf8(line));
            }
        }
        Ok(Self(table))
    }

    pub fn get(&self, key: &[u16]) -> Option<&[u16]> {
        self.0.get(key).map(Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&[u16], &[u16])> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_slice(), value.as_slice()))
    }
}

/// 按编号升序查询的字符串表；平台界面使用零，游戏使用一。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringTables(BTreeMap<u32, StringTable>);

impl StringTables {
    /// 先移除旧表，再执行读取；缺失返回 false，空文件仍安装空表。
    pub fn reload(
        &mut self,
        selector: u32,
        read: impl FnOnce() -> Result<Option<Vec<u8>>>,
    ) -> Result<bool> {
        self.0.remove(&selector);
        let Some(bytes) = read()? else {
            return Ok(false);
        };
        self.0.insert(selector, StringTable::parse(&bytes)?);
        Ok(true)
    }

    pub fn table(&self, selector: u32) -> Option<&StringTable> {
        self.0.get(&selector)
    }

    /// 仅解析开头配对的 `$…$` 或 `#…#`；缺失键返回标记后的回退文字。
    pub fn resolve<'a>(&'a self, input: &'a [u16]) -> &'a [u16] {
        let input = &input[..input
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(input.len())];
        let Some(&delimiter @ (0x24 | 0x23)) = input.first() else {
            return input;
        };
        let Some(end) = input[1..].iter().position(|&unit| unit == delimiter) else {
            return input;
        };
        let end = end + 2;
        self.0
            .values()
            .find_map(|table| table.get(&input[..end]))
            .unwrap_or(&input[end..])
    }
}

fn remove_comments(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut position = 0;
    let mut block = false;
    let mut line = false;
    while position < input.len() {
        let byte = input[position];
        if block && input[position..].starts_with(b"*/") {
            block = false;
            position += 2;
            continue;
        }
        if matches!(byte, b'\r' | b'\n') {
            output.push(byte);
            line = false;
        } else if !block && !line {
            if input[position..].starts_with(b"/*") {
                block = true;
                position += 2;
                continue;
            }
            if input[position..].starts_with(b"//") {
                line = true;
                position += 2;
                continue;
            }
            output.push(if byte == b'\t' { b' ' } else { byte });
        }
        position += 1;
    }
    output
}

fn decode_utf8(input: &[u8]) -> Vec<u16> {
    let mut output = Vec::new();
    let mut position = 0;
    while position < input.len() {
        let lead = input[position];
        position += 1;
        let length = match lead {
            0..=0x7f => {
                output.push(u16::from(lead));
                continue;
            }
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => {
                output.push(0xfffd);
                continue;
            }
        };
        // 转换入口把终止零计入剩余长度；过短的尾部整体替换。
        if input.len() + 2 - position < length {
            output.push(0xfffd);
            break;
        }
        let mut value = u32::from(lead & (0x7f >> length));
        let mut valid = true;
        for index in 1..length {
            let next = input.get(position).copied().unwrap_or(0);
            if next & 0xc0 != 0x80 {
                valid = false;
                break;
            }
            position += 1;
            value = (value << 6) | u32::from(next & 0x3f);
            // 区间错误在第二字节确认，保留其后的字节继续转换。
            if index == 1
                && matches!(
                    (lead, next),
                    (0xe0, 0x80..=0x9f)
                        | (0xed, 0xa0..=0xbf)
                        | (0xf0, 0x80..=0x8f)
                        | (0xf4, 0x90..=0xbf)
                )
            {
                valid = false;
                break;
            }
        }
        if valid {
            output.extend(
                char::from_u32(value)
                    .unwrap()
                    .encode_utf16(&mut [0; 2])
                    .iter()
                    .copied(),
            );
        } else {
            output.push(0xfffd);
        }
    }
    output
}
