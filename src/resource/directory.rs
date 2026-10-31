use super::terminated;
use crate::{Error, Result, read, text};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: Vec<u8>,
    pub offset: u32,
    pub length: u32,
}

/// 单个归档的内部目录；名称索引保留基线两种大小写转换的不同步长。
#[derive(Clone, Debug)]
pub struct ArchiveDirectory {
    entries: Vec<DirectoryEntry>,
    index: BTreeMap<Vec<u16>, usize>,
}

impl ArchiveDirectory {
    /// 条目名称为解密后的 CP932 字节，重复索引保留第一项。
    pub fn new(entries: Vec<DirectoryEntry>) -> Result<Self> {
        let mut index = BTreeMap::new();
        for (i, entry) in entries.iter().enumerate() {
            let mut name = terminated(&entry.name).to_vec();
            let mut p = 0;
            while p < name.len() {
                let lead = matches!(name[p], 0x81..=0x9f | 0xe0..=0xef);
                let trail = name
                    .get(p + 1)
                    .is_some_and(|b| matches!(b, 0x40..=0x7e | 0x80..=0xfc));
                if lead && trail {
                    p += 2;
                } else {
                    name[p].make_ascii_uppercase();
                    p += 1;
                }
            }
            let key: Vec<u16> = text::cp932(&name)?.encode_utf16().collect();
            index.entry(key).or_insert(i);
        }
        Ok(Self { entries, index })
    }

    /// 读取完整的未加密 KIF；加密目录需先解密并通过 new 提供条目。
    pub fn from_plain_kif(bytes: &[u8]) -> Result<Self> {
        if bytes.get(..4) != Some(b"KIF\0") {
            return Err(Error::new(0, "KIF 签名无效"));
        }
        let count = read::u32_at(bytes, 4)? as usize;
        let end = count
            .checked_mul(72)
            .and_then(|n| n.checked_add(8))
            .filter(|&n| n <= bytes.len() && n <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(4, "KIF 目录长度无效"))?;
        let mut entries = Vec::with_capacity(count);
        for p in (8..end).step_by(72) {
            let raw_name = &bytes[p..p + 64];
            let name = terminated(raw_name);
            if name.len() == 64 {
                return Err(Error::new(p, "KIF 条目名称缺少终止符"));
            }
            if p == 8 && name.eq_ignore_ascii_case(b"__key__.dat") {
                return Err(Error::new(p, "KIF 目录需要解密"));
            }
            let offset = read::u32_at(bytes, p + 64)?;
            let length = read::u32_at(bytes, p + 68)?;
            if (offset as usize)
                .checked_add(length as usize)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(Error::new(p + 64, "KIF 条目数据越界"));
            }
            entries.push(DirectoryEntry {
                name: name.to_vec(),
                offset,
                length,
            });
        }
        Self::new(entries)
    }

    pub fn entries(&self) -> &[DirectoryEntry] {
        &self.entries
    }

    /// 查询归档内部 CP932 名称，目录分隔符和点分量保持原值。
    pub fn find(&self, name: &[u8]) -> Result<Option<&DirectoryEntry>> {
        let name: Vec<u16> = text::cp932(terminated(name))?.encode_utf16().collect();
        Ok(self.find_utf16(&name))
    }

    /// 对应原目录查询的宽字符入口，首个 NUL 终止名称。
    pub fn find_utf16(&self, name: &[u16]) -> Option<&DirectoryEntry> {
        let mut key = name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())].to_vec();
        let mut p = 0;
        while p < key.len() {
            if key[p] & 0xff00 != 0 {
                p += 2;
            } else {
                key[p] = (key[p] as u8).to_ascii_uppercase() as u16;
                p += 1;
            }
        }
        self.index.get(&key).map(|&i| &self.entries[i])
    }
}
