use super::{Category, XmlDocument, has_directory, terminated};
use crate::{Error, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: Vec<u8>,
    pub category: Category,
    pub directory: Vec<u8>,
}

/// 启动配置中的 128 个类别槽位和 64 项必需归档编号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveConfig {
    pub entries: [ArchiveEntry; 128],
    pub required: [i32; 64],
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| ArchiveEntry::default()),
            required: [-1; 64],
        }
    }
}

impl ArchiveConfig {
    /// 接受已解码的 UTF-8 XML；字段保留 UTF-8 字节和字面实体文本。
    pub fn from_xml(input: &str) -> Result<Self> {
        let xml = XmlDocument::parse(input.as_bytes())?;
        let mut config = Self::default();
        if xml.root_name() != b"document" {
            return Ok(config);
        }
        for (index, entry) in config.entries.iter_mut().enumerate() {
            for (field, target, capacity) in [
                ("name", &mut entry.name, 32),
                ("int", &mut entry.category.prefix, 32),
                ("ext", &mut entry.category.extension, 8),
                ("dir", &mut entry.directory, 260),
            ] {
                let path = format!("INT/int{index:02}/{field}");
                if let Some(value) = xml.text(path.as_bytes())? {
                    if value.len() >= capacity {
                        return Err(Error::new(0, "资源类别字段超过基线容量"));
                    }
                    *target = value.to_vec();
                }
            }
        }
        for (index, value) in config.required.iter_mut().enumerate() {
            *value = xml.integer(format!("INT/need{index:02}").as_bytes(), u32::MAX)? as i32;
        }
        Ok(config)
    }

    /// 按名称忽略 ASCII 大小写查找，第一个相同名称的槽位优先。
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let name = terminated(name);
        self.entries
            .iter()
            .position(|entry| terminated(&entry.name).eq_ignore_ascii_case(name))
    }

    /// 带目录的名称直接返回；其余名称按显式类别补全。
    pub fn resolve(&self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        if has_directory(terminated(name)) {
            return Category::default().resolve(name);
        }
        let index = self
            .find(category)
            .ok_or_else(|| Error::new(0, "未注册的资源类别"))?;
        self.entries[index].category.resolve(name)
    }
}
