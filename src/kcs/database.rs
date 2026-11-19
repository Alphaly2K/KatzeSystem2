use super::{Interpreter, ResourceServices, strings};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

pub(super) mod quest;
pub(super) mod table;

pub(super) const SAVED_ENTRY_SIZE: usize = 268;

/// KCDB 的一个词条；字符串保留 CP932 字节。
#[derive(Debug, Default)]
pub struct DatabaseRecord {
    pub id: u32,
    pub title: Vec<u8>,
    pub short_title: Vec<u8>,
    /// IND1～IND24；缺失或空字段为 None。
    pub terms: [Option<Vec<u8>>; 24],
    pub seen: u32,
    pub pending: u32,
    pub first_seen: u32,
}

/// 普通 869 创建的词条数据库，拥有独立的已读位和更新标记。
#[derive(Debug, Default)]
pub struct Database {
    entries: BTreeMap<Vec<u8>, DatabaseRecord>,
    formatting: [Vec<u8>; 3],
    pub(super) dirty: u32,
}

fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

fn fold(name: &mut [u8]) {
    let mut at = 0;
    while at + 1 < name.len() {
        let lead = name[at];
        let trail = name[at + 1];
        if matches!(lead, 0x81..=0x9f | 0xe0..=0xef) && matches!(trail, 0x40..=0x7e | 0x80..=0xfc) {
            if lead == 0x82 {
                name[at + 1] = match trail {
                    0xaa..=0xc0 if trail.is_multiple_of(2) => trail - 1,
                    0xc3 | 0xc5 | 0xc7 => trail - 1,
                    0xce | 0xd1 | 0xd4 | 0xd7 | 0xda => trail - 1,
                    0xcf | 0xd2 | 0xd5 | 0xd8 | 0xdb => trail - 2,
                    0x9f | 0xa1 | 0xa3 | 0xa5 | 0xa7 | 0xc1 | 0xe1 | 0xe3 | 0xe5 => trail + 1,
                    _ => trail,
                };
            }
            at += 2;
        } else {
            at += 1;
        }
    }
}

fn key(name: &[u8]) -> Vec<u8> {
    let mut name = text(name).to_vec();
    fold(&mut name);
    name
}

fn first_character(bytes: &[u8]) -> u16 {
    match text(bytes) {
        [lead, trail, ..]
            if matches!(lead, 0x81..=0x9f | 0xe0..=0xef)
                && matches!(trail, 0x40..=0x7e | 0x80..=0xfc) =>
        {
            u16::from(*lead) << 8 | u16::from(*trail)
        }
        [byte, ..] => u16::from(*byte),
        [] => 0,
    }
}

fn replace_newlines(bytes: &[u8], replacement: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let part = if bytes[at..].starts_with(b"\\n") {
            at += 2;
            replacement
        } else {
            at += 1;
            &bytes[at - 1..at]
        };
        if output.len() + part.len() > read::MAX_BYTES {
            return Err(Error::new(at, "数据库文本展开超过上限"));
        }
        output.extend_from_slice(part);
    }
    Ok(output)
}

impl Database {
    /// 从完整 KCDB 容器装载；三个文本配置依次为换行替换、后缀、前缀。
    /// 非 KCDB 或解压失败保留空库，结构越界返回诊断。
    pub fn from_bytes(bytes: &[u8], formatting: [Vec<u8>; 3]) -> Result<Self> {
        let mut database = Self {
            formatting: formatting.map(|value| text(&value).to_vec()),
            ..Default::default()
        };
        let Some(table) = table::Table::parse(bytes)? else {
            return Ok(database);
        };
        let Some(id_size) = table.width(b"ID") else {
            return Ok(database);
        };
        if id_size == 0 || table.len() == 0 {
            return Ok(database);
        }
        if id_size != 4 {
            return Err(Error::new(table.header(), "数据库 ID 字段尺寸无效"));
        }
        for name in [b"INDEX".as_slice(), b"TITLE", b"TITLE_S"] {
            if table.width(name).is_none() {
                return Err(Error::new(table.header(), "数据库缺少必要文本字段"));
            }
        }
        for row in 0..table.len() {
            let cell = |name: &[u8]| table.cell(row, name).unwrap_or_default();
            let name = key(read::zstr(cell(b"INDEX"), 0)?);
            if name.len() >= 256 {
                return Err(Error::new(table.header(), "数据库词条名称超过存档字段上限"));
            }
            if database.entries.contains_key(&name) {
                continue;
            }
            let mut record = DatabaseRecord {
                id: read::u32_at(cell(b"ID"), 0)?,
                title: read::zstr(cell(b"TITLE"), 0)?.to_vec(),
                short_title: read::zstr(cell(b"TITLE_S"), 0)?.to_vec(),
                ..Default::default()
            };
            for (i, term) in record.terms.iter_mut().enumerate() {
                let bytes = cell(format!("IND{}", i + 1).as_bytes());
                let value = if bytes.is_empty() {
                    bytes
                } else {
                    read::zstr(bytes, 0)?
                };
                if !value.is_empty() {
                    *term = Some(replace_newlines(value, &database.formatting[0])?);
                }
            }
            database.entries.insert(name, record);
        }
        Ok(database)
    }

    /// 按归一化后的 CP932 名称查找，ASCII 大小写保持独立。
    pub fn record(&self, name: &[u8]) -> Option<&DatabaseRecord> {
        self.entries.get(&key(name))
    }

    /// 按归一化名称的字节序遍历。
    pub fn records(&self) -> impl Iterator<Item = (&[u8], &DatabaseRecord)> {
        self.entries
            .iter()
            .map(|(name, record)| (name.as_slice(), record))
    }

    pub fn formatting(&self) -> &[Vec<u8>; 3] {
        &self.formatting
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty != 0
    }

    pub(crate) fn find(&self, from: &[u8], to: &[u8], nth: u32) -> Option<&[u8]> {
        let (from, to) = (first_character(from), first_character(to));
        self.entries
            .keys()
            .skip_while(|name| {
                let first = first_character(name);
                first < from || first > to
            })
            // 首项之后只检查上界，保持全角与半角名称交错时的遍历顺序。
            .take_while(|name| first_character(name) <= to)
            .nth(nth as usize)
            .map(Vec::as_slice)
    }

    pub(crate) fn position(&self, name: &[u8]) -> u32 {
        let name = key(name);
        self.entries
            .keys()
            .position(|key| *key == name)
            .map_or(0, |i| i as u32 + 1)
    }

    pub(crate) fn max_appear(&self, name: &[u8]) -> u32 {
        self.record(name).map_or(0, |record| {
            record
                .terms
                .iter()
                .enumerate()
                .fold(0, |mask, (i, term)| mask | (u32::from(term.is_some()) << i))
        })
    }

    pub(crate) fn visible_terms(&self, name: &[u8]) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        if let Some(record) = self.record(name) {
            for (i, term) in record.terms.iter().enumerate() {
                if let Some(term) = term.as_ref().filter(|_| record.seen & (1 << i) != 0) {
                    let length = self.formatting[2].len() + term.len() + self.formatting[1].len();
                    if output.len() + length >= 2560 {
                        return Err(Error::new(0, "数据库索引输出超过固定缓冲区上限"));
                    }
                    output.extend_from_slice(&self.formatting[2]);
                    output.extend_from_slice(term);
                    output.extend_from_slice(&self.formatting[1]);
                }
            }
        }
        Ok(output)
    }

    pub(crate) fn clear_updates(&mut self, name: Option<&[u8]>) {
        match name {
            Some(name) => {
                if let Some(record) = self.entries.get_mut(&key(name)) {
                    record.pending = 0;
                    record.first_seen = 0;
                }
            }
            None => {
                for record in self.entries.values_mut() {
                    record.pending = 0;
                    record.first_seen = 0;
                }
            }
        }
    }

    pub(super) fn mark(&mut self, name: &[u8], bit: u32) {
        if let Some(record) = self.entries.get_mut(&key(name)) {
            if record.seen == 0 {
                record.first_seen = 1;
                self.dirty = 1;
            }
            let mask = 1u32 << (bit & 31);
            if record.seen & mask == 0 {
                record.seen |= mask;
                record.pending |= mask;
                self.dirty = 1;
            }
        }
    }

    pub(super) fn saved_size(&self) -> Result<usize> {
        let size = 4 + self.entries.len() * SAVED_ENTRY_SIZE;
        if size > read::MAX_BYTES {
            return Err(Error::new(0, "数据库存档超过上限"));
        }
        Ok(size)
    }

    pub(super) fn export(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![0; self.saved_size()?];
        bytes[..4].copy_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for ((name, record), entry) in self
            .entries
            .iter()
            .zip(bytes[4..].chunks_exact_mut(SAVED_ENTRY_SIZE))
        {
            entry[..name.len()].copy_from_slice(name);
            for (i, word) in [record.seen, record.pending, record.first_seen]
                .into_iter()
                .enumerate()
            {
                entry[256 + i * 4..260 + i * 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        Ok(bytes)
    }

    pub(super) fn import(&mut self, bytes: &mut [u8]) -> Result<()> {
        let count = read::u32_at(bytes, 0)? as usize;
        let size = saved_size(count)?;
        read::range(bytes, 0, size)?;
        for entry in bytes[4..size].chunks_exact_mut(SAVED_ENTRY_SIZE) {
            let end = read::zstr(&entry[..256], 0)?.len();
            // 导入也归一化来源缓冲区，未知名称仍保留这一修改。
            fold(&mut entry[..end]);
            if let Some(record) = self.entries.get_mut(&entry[..end]) {
                record.seen = read::u32_at(entry, 256)?;
                record.pending = read::u32_at(entry, 260)?;
                record.first_seen = read::u32_at(entry, 264)?;
            }
        }
        Ok(())
    }
}

pub(super) fn saved_size(count: usize) -> Result<usize> {
    count
        .checked_mul(SAVED_ENTRY_SIZE)
        .and_then(|n| n.checked_add(4))
        .filter(|&n| n <= read::MAX_BYTES && count <= read::MAX_ITEMS)
        .ok_or_else(|| Error::new(0, "数据库导入数量超过上限"))
}

pub(crate) fn load(
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<Database> {
    let args = super::native::words(arguments, 4, vm.pc)?;
    let mut values = Vec::with_capacity(4);
    for address in args {
        values.push(strings::format(vm, address, 0)?);
    }
    if text(&values[0]).len() >= 260 || values[1..].iter().any(|value| text(value).len() >= 32) {
        return Err(Error::new(vm.pc, "数据库装载文本超过固定缓冲区上限"));
    }
    let services = services.ok_or_else(|| Error::new(vm.pc, "数据库装载需要资源服务"))?;
    let name = services.resolve(b"database", text(&values[0]))?;
    let bytes = services.read(&name, ResourceMode::LooseFirst)?;
    let formatting = [values[1].clone(), values[2].clone(), values[3].clone()];
    Database::from_bytes(bytes.as_deref().unwrap_or_default(), formatting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_walk_keeps_halfwidth_names_after_the_first_fullwidth_match() {
        let mut database = Database::default();
        for name in [b"A".as_slice(), b"\x82\xa9", b"\x82\xe2", b"\xa6half"] {
            database
                .entries
                .insert(name.to_vec(), DatabaseRecord::default());
        }
        assert_eq!(
            database.find(b"\x82\xa9", b"\x82\xfc", 2),
            Some(b"\xa6half".as_slice())
        );
        assert_eq!(database.find(b"\x82\xaa", b"\x82\xaa", 0), None);
        assert_eq!(database.find(b"z", b"A", 0), None);
        let record = database.entries.get_mut(b"A".as_slice()).unwrap();
        record.terms[0] = Some(Vec::new());
        record.seen = 1;
        database.formatting = [Vec::new(), b"]".to_vec(), b"[".to_vec()];
        assert_eq!(database.max_appear(b"A"), 1);
        assert_eq!(database.visible_terms(b"A").unwrap(), b"[]");
    }
}
