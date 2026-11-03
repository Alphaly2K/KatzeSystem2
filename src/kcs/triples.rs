use crate::{Error, Result, read, text};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct TripleTable {
    groups: BTreeMap<Vec<u8>, [u32; 3]>,
    aliases: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl TripleTable {
    pub fn parse(input: &[u8]) -> Result<Self> {
        if input.len() > read::MAX_BYTES {
            return Err(Error::new(0, "三整数分组表超过读取上限"));
        }
        let input = &input[..input.iter().position(|&b| b == 0).unwrap_or(input.len())];
        let mut table = Self::default();
        let mut group = b"__UNKNOWN__".to_vec();
        let mut values = [100; 3];
        let mut block_comment = false;
        for raw in input.split(|&b| b == b'\r' || b == b'\n') {
            let mut line = Vec::new();
            let mut p = 0;
            while p < raw.len() {
                if block_comment {
                    if raw[p..].starts_with(b"*/") {
                        block_comment = false;
                        p += 2;
                    } else {
                        p += 1;
                    }
                } else if raw[p..].starts_with(b"//") {
                    break;
                } else if raw[p..].starts_with(b"/*") {
                    block_comment = true;
                    p += 2;
                } else {
                    line.push(raw[p]);
                    p += 1;
                }
            }
            line.retain(|&b| b != b' ' && b != b'\t');
            if line.is_empty() {
                continue;
            }
            if let Some(hash) = line.iter().position(|&b| b == b'#') {
                if let Some(open) = line[hash..].iter().position(|&b| b == b'(') {
                    group = key(&line[hash..hash + open], 511)?;
                    let mut remaining = &line[hash + open + 1..];
                    // 不完整的头部仍切换组，已完成的分量逐项继承到下一行。
                    for (value, delimiter) in values.iter_mut().zip(*b",,)") {
                        let Some(end) = remaining.iter().position(|&b| b == delimiter) else {
                            break;
                        };
                        if end >= 512 {
                            return Err(Error::new(0, "三整数分组表字段过长"));
                        }
                        *value =
                            text::integer_prefix(&remaining[..end]).map_or(0, |(_, number)| number);
                        remaining = &remaining[end + 1..];
                    }
                    continue;
                }
            }
            let alias = key(&line, 511)?;
            let additional = usize::from(!table.groups.contains_key(&group))
                + usize::from(!table.aliases.contains_key(&alias));
            if table.groups.len() + table.aliases.len() + additional > read::MAX_ITEMS {
                return Err(Error::new(0, "三整数分组表条目超过上限"));
            }
            table.groups.entry(group.clone()).or_insert(values);
            table.aliases.entry(alias).or_insert_with(|| group.clone());
        }
        Ok(table)
    }

    pub fn query(&self, name: &[u8], alias: bool) -> Result<Option<[u32; 3]>> {
        let key = key(name, if alias { 1023 } else { 511 })?;
        let name = if alias {
            let Some(name) = self.aliases.get(&key) else {
                return Ok(None);
            };
            name
        } else {
            &key
        };
        Ok(self.groups.get(name).copied())
    }
}

fn key(input: &[u8], limit: usize) -> Result<Vec<u8>> {
    let input = &input[..input.iter().position(|&b| b == 0).unwrap_or(input.len())];
    if input.len() > limit {
        return Err(Error::new(0, "三整数分组表名称过长"));
    }
    let mut output = input.to_vec();
    let mut p = 0;
    while p < output.len() {
        let width = super::strings::character_width(&output[p..]);
        if width == 1 {
            output[p] = output[p].to_ascii_uppercase();
        }
        p += width;
    }
    Ok(output)
}
