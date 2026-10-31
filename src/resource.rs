use crate::{Error, Result, read};

mod config;
mod directory;
mod kif;
mod router;
mod xml;
pub use config::{ArchiveConfig, ArchiveEntry};
pub use directory::{ArchiveDirectory, DirectoryEntry};
pub use kif::{KifArchive, verification_seed};
pub use router::{
    ArchiveMount, ResourceData, ResourceGroup, ResourceMember, ResourceMode, ResourceRouter,
    ResourceSlot, ResourceSource,
};
pub use xml::XmlDocument;

/// 由资源配置提供的虚拟名前缀和缺省扩展名；目录扫描使用基线 CP932 范围。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Category {
    pub prefix: Vec<u8>,
    pub extension: Vec<u8>,
}

impl Category {
    pub fn resolve(&self, name: &[u8]) -> Result<Vec<u8>> {
        let name = terminated(name);
        if has_directory(name) {
            if name.len() > read::MAX_BYTES {
                return Err(Error::new(0, "资源名称过长"));
            }
            return Ok(name.to_vec());
        }
        let prefix = terminated(&self.prefix);
        let extension = terminated(&self.extension);
        let has_extension = name
            .rsplit(|b| matches!(b, b'/' | b'\\'))
            .next()
            .unwrap()
            .contains(&b'.');
        let prefix_length = if prefix.is_empty() {
            0
        } else {
            prefix.len() + 1
        };
        let extension_length = if has_extension {
            0
        } else {
            extension.len() + 1
        };
        let length = name
            .len()
            .checked_add(prefix_length)
            .and_then(|n| n.checked_add(extension_length))
            .filter(|&n| n <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(0, "资源名称过长"))?;
        let mut result = Vec::with_capacity(length);
        if !prefix.is_empty() {
            result.extend(prefix);
            result.push(b'/');
        }
        result.extend(name);
        if !has_extension {
            result.push(b'.');
            result.extend(extension);
        }
        Ok(result)
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

fn has_directory(name: &[u8]) -> bool {
    let mut last_separator = 0;
    let mut i = 0;
    while i < name.len() {
        let lead = matches!(name[i], 0x81..=0x9f | 0xe0..=0xef);
        let trail = name
            .get(i + 1)
            .is_some_and(|b| matches!(b, 0x40..=0x7e | 0x80..=0xfc));
        if lead && trail {
            i += 2;
        } else {
            if matches!(name[i], b'/' | b'\\') {
                last_separator = i;
            }
            i += 1;
        }
    }
    last_separator != 0
}
