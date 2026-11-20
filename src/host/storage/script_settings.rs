use crate::{Error, Result, read};
use std::collections::BTreeMap;

const MAGIC: &[u8; 8] = b"KCSSET01";

/// Host-owned settings shared by scripts within one runtime.
#[derive(Default)]
pub struct ScriptSettings {
    groups: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    record_bytes: usize,
}

impl ScriptSettings {
    /// Maximum encoded snapshot size in bytes.
    pub const MAX_SNAPSHOT_BYTES: usize = read::MAX_BYTES;

    /// Load a snapshot produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > Self::MAX_SNAPSHOT_BYTES || !bytes.starts_with(MAGIC) {
            return Err(Error::new(0, "无效的脚本设置文件"));
        }
        let mut settings = Self::default();
        let mut offset = MAGIC.len();
        while offset != bytes.len() {
            let mut field = || {
                let size = read::sized(read::u32_at(bytes, offset)?, offset)?;
                offset += 4;
                let field = read::range(bytes, offset, size)?;
                offset += size;
                Ok::<_, Error>(field)
            };
            let group = decode_key(field()?)?;
            let name = decode_key(field()?)?;
            let data = field()?;
            settings.record_bytes = settings.next_size(&group, &name, data.len())?;
            if settings
                .groups
                .entry(group)
                .or_default()
                .insert(name, data.to_vec())
                .is_some()
            {
                return Err(Error::new(offset, "脚本设置键重复"));
            }
        }
        Ok(settings)
    }

    /// Encode the complete snapshot, keeping setting values as raw bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(MAGIC.len() + self.record_bytes);
        result.extend(MAGIC);
        for (group, entries) in &self.groups {
            for (name, bytes) in entries {
                for field in [group.as_bytes(), name.as_bytes(), bytes.as_slice()] {
                    result.extend((field.len() as u32).to_le_bytes());
                    result.extend(field);
                }
            }
        }
        result
    }

    pub(crate) fn group(&self, name: &str) -> Option<&BTreeMap<String, Vec<u8>>> {
        self.groups.get(name)
    }

    pub(crate) fn put(
        &mut self,
        group: String,
        name: String,
        bytes: Vec<u8>,
        persist: impl FnOnce(&Self) -> Result<bool>,
    ) -> Result<bool> {
        let previous_size = self.record_bytes;
        self.record_bytes = self.next_size(&group, &name, bytes.len())?;
        let old = self
            .groups
            .entry(group.clone())
            .or_default()
            .insert(name.clone(), bytes);
        let result = persist(self);
        if !matches!(result, Ok(true)) {
            self.record_bytes = previous_size;
            let entries = self.groups.get_mut(&group).unwrap();
            if let Some(previous) = old {
                entries.insert(name, previous);
            } else {
                entries.remove(&name);
                if entries.is_empty() {
                    self.groups.remove(&group);
                }
            }
        }
        result
    }

    fn next_size(&self, group: &str, name: &str, length: usize) -> Result<usize> {
        let header = 12 + group.len() + name.len();
        let old = self.groups.get(group).and_then(|entries| entries.get(name));
        let size =
            self.record_bytes - old.map_or(0, |bytes| header + bytes.len()) + header + length;
        if size > Self::MAX_SNAPSHOT_BYTES - MAGIC.len() {
            return Err(Error::new(0, "脚本设置文件超过读取上限"));
        }
        Ok(size)
    }
}

pub(crate) fn fold_key(key: &str) -> String {
    key.chars().flat_map(char::to_uppercase).collect()
}

fn decode_key(bytes: &[u8]) -> Result<String> {
    let key = std::str::from_utf8(bytes).map_err(|_| Error::new(0, "脚本设置键不是 UTF-8"))?;
    Ok(fold_key(key))
}
