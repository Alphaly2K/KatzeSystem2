//! `soundconf.xml` 的 SOUNDGROUP、MASTER 与 PART 段读取。
//!
//! 该文件与启动 XML 分开：声音分类树、声音组范围和主音量都在此声明。
//! 读取只还原配置字段，不合并用户设置，也不判断分类范围是否可用。

use super::{SoundConfig, SoundGroup, SoundGroups, SoundPart};
use crate::{Error, Result, read, text};
use roxmltree::{Document, Node, ParsingOptions};

/// 分类树的最大嵌套深度；正常配置不超过四层。
const MAX_DEPTH: usize = 32;

impl SoundConfig {
    /// 读取 `soundconf.xml` 的 MASTER 与 PART 段。
    ///
    /// 主音量、主播放开关与主静音来自 MASTER；PART 的每个子节点为一个分类，
    /// 其 `<part>` 子节点的子节点递归为有序子分类。`<head>` 保存为
    /// `file_prefix`，`<int>` 保存为 `archive`；顶层分类缺失字段保留
    /// `SoundPart` 的未设置值，子分类缺失字段逐字段沿用父级值。`comment`、
    /// `sameid` 与 `group` 不进入分类树，分别由文件命名、调用方和声音组处理。
    pub fn from_xml(bytes: &[u8]) -> Result<Self> {
        let input = prepare(bytes, "声音配置")?;
        let document = Document::parse_with_options(
            &input,
            ParsingOptions {
                nodes_limit: read::MAX_ITEMS as u32,
                ..Default::default()
            },
        )
        .map_err(|error| Error::new(0, format!("声音配置 XML 无效：{error}")))?;
        let root = document.root_element();
        let master = element_child(root, "MASTER");
        let mut parts = Vec::new();
        if let Some(container) = element_child(root, "PART") {
            for node in element_children(container) {
                parts.push(parse_part(node, 0, None)?);
            }
        }
        Ok(Self {
            parts,
            master_volume: master
                .and_then(|node| element_child(node, "volume"))
                .map(|node| integer(node, -1)),
            master_play_enabled: master
                .and_then(|node| element_child(node, "play"))
                .map(|node| integer(node, -1)),
            master_system_voice: master
                .and_then(|node| element_child(node, "sysplay"))
                .map(|node| integer(node, -1)),
            master_mute: master
                .and_then(|node| element_child(node, "mute"))
                .map_or(-1, |node| integer(node, -1)),
        })
    }
}

impl SoundGroups {
    /// 读取 `soundconf.xml` 的 SOUNDGROUP 段。
    ///
    /// 每个子节点的 `<id>`、`<label>`、`<min>`、`<max>` 分别给出分类编号、
    /// 原始标签和包含两端的物理通道范围；新配置的表键与分类编号相同。
    /// `<id>` 缺失按零处理，编号重复的条目按文件顺序保留。
    pub fn from_xml(bytes: &[u8]) -> Result<Self> {
        let input = prepare(bytes, "声音配置")?;
        let document = Document::parse_with_options(
            &input,
            ParsingOptions {
                nodes_limit: read::MAX_ITEMS as u32,
                ..Default::default()
            },
        )
        .map_err(|error| Error::new(0, format!("声音配置 XML 无效：{error}")))?;
        let mut entries = Vec::new();
        if let Some(container) = element_child(document.root_element(), "SOUNDGROUP") {
            for node in element_children(container) {
                let id = element_child(node, "id").map_or(0, |field| integer(field, 0)) as u32;
                entries.push(SoundGroup {
                    key: id,
                    id,
                    label: field_bytes(node, "label"),
                    range: [
                        element_child(node, "min").map_or(0, |field| integer(field, 0)),
                        element_child(node, "max").map_or(0, |field| integer(field, 0)),
                    ],
                });
            }
        }
        Ok(Self { entries })
    }
}

fn parse_part(node: Node<'_, '_>, depth: usize, parent: Option<&SoundPart>) -> Result<SoundPart> {
    if depth >= MAX_DEPTH {
        return Err(Error::new(0, "声音分类嵌套超过上限"));
    }
    let mut part = SoundPart {
        name: field_bytes(node, "name"),
        type_name: field_bytes(node, "type"),
        file_prefix: field_bytes(node, "head"),
        archive: field_bytes(node, "int"),
        id: field_integer(node, "id"),
        master_volume: field_integer(node, "mastervol"),
        volume: field_integer(node, "volume"),
        voice_volume: field_integer(node, "voicevol"),
        skip_volume: field_integer(node, "skipvol"),
        system_voice: field_integer(node, "sysplay"),
        play_enabled: field_integer(node, "play"),
        mute: field_integer(node, "mute"),
        children: Vec::new(),
    };
    if let Some(parent) = parent {
        inherit(&mut part, parent);
    }
    if let Some(container) = element_child(node, "part") {
        for child in element_children(container) {
            let parsed = parse_part(child, depth + 1, Some(&part))?;
            part.children.push(parsed);
        }
    }
    Ok(part)
}

/// 子分类未给出的字段沿用父级值；显式零值不是缺省。
fn inherit(part: &mut SoundPart, parent: &SoundPart) {
    for (value, parent_value) in [
        (&mut part.name, &parent.name),
        (&mut part.type_name, &parent.type_name),
        (&mut part.file_prefix, &parent.file_prefix),
        (&mut part.archive, &parent.archive),
    ] {
        if value.is_empty() {
            value.clone_from(parent_value);
        }
    }
    for (value, parent_value) in [
        (&mut part.id, parent.id),
        (&mut part.master_volume, parent.master_volume),
        (&mut part.volume, parent.volume),
        (&mut part.voice_volume, parent.voice_volume),
        (&mut part.skip_volume, parent.skip_volume),
        (&mut part.system_voice, parent.system_voice),
        (&mut part.play_enabled, parent.play_enabled),
        (&mut part.mute, parent.mute),
    ] {
        if *value == -1 {
            *value = parent_value;
        }
    }
}

/// 读取 UTF-8 正文，丢弃缩进与末尾换行，保留内部空格和字面实体。
fn prepare(bytes: &[u8], label: &str) -> Result<String> {
    if bytes.len() > read::MAX_BYTES {
        return Err(Error::new(0, format!("{label} XML 超过读取上限")));
    }
    let bytes = &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())];
    let input = std::str::from_utf8(bytes)
        .map_err(|_| Error::new(0, format!("{label} XML 需要 UTF-8 文本")))?;
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let body = match input.strip_prefix("<?xml") {
        Some(rest) => {
            let end = rest
                .find("?>")
                .ok_or_else(|| Error::new(0, format!("{label} XML 声明未结束")))?;
            &rest[end + 2..]
        }
        None => input,
    };
    // 与基线 XML 一致：先保留字面实体和单独 CR，再交给语法解析器。
    let normalized = body
        .replace("\r\n", "\n")
        .replace('&', "&amp;")
        .replace('\r', "&#13;");
    if normalized.len() > read::MAX_BYTES {
        return Err(Error::new(0, format!("{label} XML 展开超过读取上限")));
    }
    Ok(normalized)
}

fn element_child<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

fn element_children<'a>(node: Node<'a, 'a>) -> impl Iterator<Item = Node<'a, 'a>> {
    node.children().filter(Node::is_element)
}

fn field_bytes(node: Node<'_, '_>, name: &str) -> Vec<u8> {
    element_child(node, name).map_or_else(Vec::new, |field| {
        let mut bytes: Vec<u8> = field
            .children()
            .filter(Node::is_text)
            .filter_map(|child| child.text())
            .flat_map(str::bytes)
            .filter(|byte| !matches!(byte, b'\r' | b'\n' | b'\t'))
            .collect();
        let start = bytes
            .iter()
            .position(|&byte| byte != b' ')
            .unwrap_or(bytes.len());
        let end = bytes
            .iter()
            .rposition(|&byte| byte != b' ')
            .map_or(start, |position| position + 1);
        bytes.drain(end..);
        bytes.drain(..start);
        bytes
    })
}

fn field_integer(node: Node<'_, '_>, name: &str) -> i32 {
    element_child(node, name).map_or(-1, |field| integer(field, -1))
}

/// 缺失或非法正文按零处理；负号按 DWORD 回绕保存。
fn integer(node: Node<'_, '_>, default: i32) -> i32 {
    let bytes: Vec<u8> = node
        .children()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .flat_map(str::bytes)
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if bytes.is_empty() {
        return default;
    }
    text::integer_prefix(&bytes).map_or(0, |(_, value)| value as i32)
}
