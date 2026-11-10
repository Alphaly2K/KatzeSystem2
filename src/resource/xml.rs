use super::terminated;
use crate::{Error, Result, read, text};
use roxmltree::Document;
use std::collections::BTreeMap;

mod write;
pub use write::XmlWriter;

/// 基线 XML 的只读字段表；重复路径合并，文本保留 UTF-8 和字面实体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlDocument {
    root: Vec<u8>,
    fields: BTreeMap<Vec<Vec<u8>>, Vec<u8>>,
}

impl XmlDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "XML 超过读取上限"));
        }
        let input = std::str::from_utf8(terminated(bytes))
            .map_err(|_| Error::new(0, "XML 需要 UTF-8 文本"))?;
        let declaration = input
            .strip_prefix("<?xml ")
            .and_then(|s| s.split_once("?>"))
            .ok_or_else(|| Error::new(0, "XML 需要起始声明"))?;
        let header = format!("<xml {}/>", declaration.0);
        let header = Document::parse(&header).map_err(xml_error)?;
        if header.root_element().attribute("version") != Some("1.0")
            || !header
                .root_element()
                .attribute("encoding")
                .is_some_and(|encoding| encoding.eq_ignore_ascii_case("UTF-8"))
        {
            return Err(Error::new(0, "XML 需要 1.0 和 UTF-8 声明"));
        }
        // 先保留字面实体和单独 CR，再让 XML 解析器处理结构。
        let normalized = input
            .replace("\r\n", "\n")
            .replace('&', "&amp;")
            .replace('\r', "&#13;");
        if normalized.len() > read::MAX_BYTES {
            return Err(Error::new(0, "XML 展开超过读取上限"));
        }
        let syntax = first_root(&normalized)?;
        let document = Document::parse_with_options(
            &syntax,
            roxmltree::ParsingOptions {
                nodes_limit: read::MAX_ITEMS as u32,
                ..Default::default()
            },
        )
        .map_err(xml_error)?;
        let root = document.root_element();
        let raw_name = |node: roxmltree::Node<'_, '_>| {
            normalized[node.range().start + 1..]
                .split([' ', '\t', '\r', '\n', '/', '>'])
                .next()
                .unwrap()
                .as_bytes()
                .to_vec()
        };
        let root_name = raw_name(root);
        let mut fields = BTreeMap::new();
        let mut key_bytes = 0usize;
        for node in document.descendants().filter(roxmltree::Node::is_element) {
            let mut path: Vec<_> = node
                .ancestors()
                .filter(roxmltree::Node::is_element)
                .map(raw_name)
                .collect();
            path.reverse();
            if root_name == b"document" {
                path.remove(0);
            }
            if path.is_empty() {
                continue;
            }
            key_bytes += path.iter().map(Vec::len).sum::<usize>();
            if key_bytes > read::MAX_BYTES {
                return Err(Error::new(0, "XML 路径存储超过上限"));
            }
            let value = node
                .children()
                .filter(roxmltree::Node::is_text)
                .filter_map(|child| child.text())
                .flat_map(str::bytes)
                .map(|byte| {
                    if matches!(byte, b'\n' | b'\t') {
                        b' '
                    } else {
                        byte
                    }
                })
                .collect();
            fields.insert(path, value);
        }
        Ok(Self {
            root: root_name,
            fields,
        })
    }

    pub fn root_name(&self) -> &[u8] {
        &self.root
    }

    /// 区分大小写，忽略空分段；每段至多 511 字节，首零结束。
    pub fn text(&self, path: &[u8]) -> Result<Option<&[u8]>> {
        let path = terminated(path);
        if path.len() > read::MAX_BYTES {
            return Err(Error::new(0, "XML 路径超过读取上限"));
        }
        let mut parts = Vec::new();
        for part in path.split(|&byte| byte == b'/') {
            if part.len() >= 512 {
                return Err(Error::new(0, "XML 路径分段超过基线容量"));
            }
            if !part.is_empty() {
                if parts.len() == read::MAX_ITEMS {
                    return Err(Error::new(0, "XML 路径分段数量超过上限"));
                }
                parts.push(part.to_vec());
            }
        }
        Ok(self.fields.get(&parts).map(Vec::as_slice))
    }

    /// 缺失字段保留默认值；存在但不含合法整数前缀的字段返回零。
    pub fn integer(&self, path: &[u8], default: u32) -> Result<u32> {
        Ok(self.text(path)?.map_or(default, |value| {
            text::integer_prefix(value).map_or(0, |(_, number)| number)
        }))
    }

    /// 缺失时保留默认值；存在时按配置数值规则截取，拒绝前导空白和加号。
    pub fn float(&self, path: &[u8], default: f32) -> Result<f32> {
        self.text(path)?.map_or(Ok(default), config_float)
    }

    /// 宽字符配置入口；每个 UTF-8 字符块保留首个 UTF-16 单元，缺失为 None。
    pub fn wide_text(&self, path: &[u8]) -> Result<Option<Vec<u16>>> {
        self.text(path)?.map(text::wide_config).transpose()
    }
}

fn config_float(value: &[u8]) -> Result<f32> {
    let value = terminated(value);
    if !matches!(value.first(), Some(b'-' | b'0'..=b'9')) {
        return Ok(0.0);
    }
    let negative = value.first() == Some(&b'-');
    let value = &value[usize::from(negative)..];
    let digits = value.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut end = digits;
    if value.get(end) == Some(&b'.') {
        end += 1;
        if !matches!(value.get(end), Some(b'f' | b'F')) {
            // 小数点后先复制一字节，再复制后续数字；该字节也可能是指数标记或零。
            end += 1;
            end += value
                .get(end..)
                .unwrap_or_default()
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        }
    }
    if end >= 1024 {
        return Err(Error::new(0, "XML 浮点文本超出转换缓冲范围"));
    }
    let bits = text::float32(&value[..end.min(value.len())]);
    Ok(f32::from_bits(bits ^ (u32::from(negative) << 31)))
}

fn xml_error(error: roxmltree::Error) -> Error {
    Error::new(0, format!("XML 无效：{error}"))
}

fn first_root(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut syntax = bytes.to_vec();
    let mut position = 0;
    let mut depth = 0usize;
    while position < bytes.len() {
        if bytes[position] != b'<' {
            position += 1;
            continue;
        }
        let tail = &input[position..];
        let delimiter = if tail.starts_with("<!--") {
            Some("-->")
        } else if tail.starts_with("<?") {
            Some("?>")
        } else {
            None
        };
        if let Some(delimiter) = delimiter {
            position += tail
                .find(delimiter)
                .ok_or_else(|| Error::new(position, "XML 标记未结束"))?
                + delimiter.len();
            continue;
        }
        if tail.starts_with("<!") {
            return Err(Error::new(position, "基线 XML 不接受此声明或 CDATA"));
        }
        let start = position;
        let closing = tail.starts_with("</");
        let mut quote = None;
        position += 1;
        while position < bytes.len() {
            let byte = bytes[position];
            if let Some(end) = quote {
                if byte == end {
                    quote = None;
                }
            } else {
                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'>' => break,
                    // 原读取器将带冒号的名字视作字面名称，不解析命名空间。
                    b':' => syntax[position] = b'_',
                    _ => {}
                }
            }
            position += 1;
        }
        if position == bytes.len() {
            return Err(Error::new(start, "XML 标签未结束"));
        }
        let empty = bytes[position - 1] == b'/';
        position += 1;
        if empty && depth != 0 {
            return Err(Error::new(start, "基线 XML 不接受嵌套自闭合节点"));
        }
        if closing {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| Error::new(start, "XML 闭合标签没有起始标签"))?;
        } else if !empty {
            depth += 1;
        }
        if depth == 0 {
            syntax.truncate(position);
            return Ok(String::from_utf8(syntax).unwrap());
        }
    }
    Err(Error::new(position, "XML 缺失完整根节点"))
}
