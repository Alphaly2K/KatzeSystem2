use super::{Category, has_directory, terminated};
use crate::{Error, Result, read, text};
use roxmltree::{Document, Node};

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
        if input.len() > read::MAX_BYTES {
            return Err(Error::new(0, "资源配置超过上限"));
        }
        let declaration = input
            .strip_prefix("<?xml ")
            .and_then(|s| s.split_once("?>"))
            .ok_or_else(|| Error::new(0, "资源配置需要 XML 声明"))?;
        let header = format!("<xml {}/>", declaration.0);
        let header = Document::parse(&header).map_err(xml_error)?;
        if header.root_element().attribute("version") != Some("1.0")
            || header.root_element().attribute("encoding") != Some("UTF-8")
        {
            return Err(Error::new(0, "资源配置需要 XML 1.0 和 UTF-8 声明"));
        }
        if input.contains("<![CDATA[") {
            return Err(Error::new(0, "基线配置不接受 CDATA"));
        }
        // 原读取器保留实体字面值和单独 CR，仅将 CRLF、LF、Tab 换为空格。
        let normalized = input
            .replace("\r\n", "\n")
            .replace('&', "&amp;")
            .replace('\r', "&#13;");
        if normalized.len() > read::MAX_BYTES {
            return Err(Error::new(0, "资源配置展开超过上限"));
        }
        let document = Document::parse(&normalized).map_err(xml_error)?;
        let root = document.root_element();
        if document.descendants().any(|node| {
            node.is_element() && node != root && normalized[node.range()].ends_with("/>")
        }) {
            return Err(Error::new(0, "基线配置不接受嵌套自闭合节点"));
        }
        let mut config = Self::default();
        if !named(root, "document") {
            return Ok(config);
        }
        for group in root.children().filter(|&node| named(node, "INT")) {
            for node in group.children().filter(Node::is_element) {
                let name = node.tag_name().name();
                if !named(node, name) {
                    continue;
                }
                if let Some(index) = slot(name, "int", 128) {
                    let entry = &mut config.entries[index];
                    for field in node.children().filter(Node::is_element) {
                        let (target, capacity) = match field.tag_name().name() {
                            "name" if named(field, "name") => (&mut entry.name, 32),
                            "int" if named(field, "int") => (&mut entry.category.prefix, 32),
                            "ext" if named(field, "ext") => (&mut entry.category.extension, 8),
                            "dir" if named(field, "dir") => (&mut entry.directory, 260),
                            _ => continue,
                        };
                        let value = direct_text(field);
                        if value.len() >= capacity {
                            return Err(Error::new(0, "资源类别字段超过基线容量"));
                        }
                        *target = value;
                    }
                } else if let Some(index) = slot(name, "need", 64) {
                    config.required[index] = text::integer_prefix(&direct_text(node))
                        .map(|(_, n)| n as i32)
                        .unwrap_or(0);
                }
            }
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

fn slot(name: &str, prefix: &str, capacity: usize) -> Option<usize> {
    let index: usize = name.strip_prefix(prefix)?.parse().ok()?;
    (index < capacity && name == format!("{prefix}{index:02}")).then_some(index)
}

fn named(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.document().input_text()[node.range().start + 1..]
            .split([' ', '\t', '\r', '\n', '/', '>'])
            .next()
            == Some(name)
}

fn direct_text(node: Node<'_, '_>) -> Vec<u8> {
    node.children()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .flat_map(str::bytes)
        .map(|b| if matches!(b, b'\n' | b'\t') { b' ' } else { b })
        .collect()
}

fn xml_error(error: roxmltree::Error) -> Error {
    Error::new(0, format!("资源配置 XML 无效：{error}"))
}
