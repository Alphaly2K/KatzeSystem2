use crate::{Error, Result, read, resource::terminated};

#[derive(Clone, Debug)]
struct Node {
    name: String,
    value: Vec<u8>,
    children: Vec<usize>,
}

/// 基线配置 XML 写出器；新节点置于同级首位，文本按字面字节输出，不转义实体。
#[derive(Clone, Debug)]
pub struct XmlWriter {
    nodes: Vec<Node>,
    bytes: usize,
}

impl Default for XmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlWriter {
    /// 建立隐式 document 根节点。
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                name: "document".into(),
                value: Vec::new(),
                children: Vec::new(),
            }],
            bytes: 8,
        }
    }

    /// 按名称分段写入字段；空路径选择根，名称和值均在首零截止。
    /// 已有字段保留同级位置；有子节点时，序列化只输出子节点。
    pub fn write(&mut self, path: &[&str], value: &[u8]) -> Result<()> {
        let value = terminated(value);
        let mut existing = Some(0);
        let mut added_nodes = 0;
        let mut added_bytes = value.len();
        for name in path {
            let name = name.split('\0').next().unwrap();
            existing = existing.and_then(|id| self.child(id, name));
            if existing.is_none() {
                added_nodes += 1;
                added_bytes = added_bytes
                    .checked_add(name.len())
                    .ok_or_else(|| Error::new(0, "XML 写出存储超过上限"))?;
            }
        }
        let replaced = existing.map_or(0, |id| self.nodes[id].value.len());
        let bytes = self
            .bytes
            .checked_sub(replaced)
            .and_then(|bytes| bytes.checked_add(added_bytes))
            .filter(|&bytes| bytes <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(0, "XML 写出存储超过上限"))?;
        if added_nodes > read::MAX_ITEMS.saturating_sub(self.nodes.len()) {
            return Err(Error::new(0, "XML 写出节点超过上限"));
        }
        let mut id = 0;
        for name in path {
            let name = name.split('\0').next().unwrap();
            id = if let Some(child) = self.child(id, name) {
                child
            } else {
                let child = self.nodes.len();
                self.nodes.push(Node {
                    name: name.into(),
                    value: Vec::new(),
                    children: Vec::new(),
                });
                self.nodes[id].children.insert(0, child);
                child
            };
        }
        self.nodes[id].value = value.to_vec();
        self.bytes = bytes;
        Ok(())
    }

    /// DWORD 按带符号十进制写出。
    pub fn write_integer(&mut self, path: &[&str], value: u32) -> Result<()> {
        self.write(path, (value as i32).to_string().as_bytes())
    }

    /// float 提升到 double 后以六位小数写出，保留负零与非有限值的符号。
    pub fn write_float(&mut self, path: &[&str], value: f32) -> Result<()> {
        self.write(path, &crate::text::format_float(value, 6))
    }

    /// 逐 UTF-16 单元转换，代理单元各自替换为 U+FFFD，FFFF 写两个 FF 字节。
    /// 首零截止；超过原转换缓冲容量时返回诊断，保留字段原值。
    pub fn write_wide(&mut self, path: &[&str], value: &[u16]) -> Result<()> {
        let mut bytes = Vec::new();
        for &unit in value.iter().take_while(|&&unit| unit != 0) {
            if unit == u16::MAX {
                bytes.extend([0xff; 2]);
            } else {
                bytes.extend(
                    char::from_u32(u32::from(unit))
                        .unwrap_or(char::REPLACEMENT_CHARACTER)
                        .encode_utf8(&mut [0; 4])
                        .as_bytes(),
                );
            }
            if bytes.len() >= 4096 {
                return Err(Error::new(0, "XML 文字超出 UTF-8 转换缓冲范围"));
            }
        }
        self.write(path, &bytes)
    }

    /// 输出 UTF-8 声明、两空格缩进及 CRLF；不包含末尾零。
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n".to_vec();
        let mut pending = vec![(0, 0, false)];
        while let Some((id, depth, closing)) = pending.pop() {
            let node = &self.nodes[id];
            let indent = depth * 2;
            let leaf = !closing && node.children.is_empty();
            let length = indent
                + if closing {
                    node.name.len() + 5
                } else if leaf {
                    node.name.len() * 2 + node.value.len() + 7
                } else {
                    node.name.len() + 4
                };
            if length > read::MAX_BYTES.saturating_sub(bytes.len()) {
                return Err(Error::new(0, "XML 输出超过读取上限"));
            }
            bytes.resize(bytes.len() + indent, b' ');
            bytes.extend(if closing { b"</".as_slice() } else { b"<" });
            bytes.extend(node.name.as_bytes());
            bytes.push(b'>');
            if leaf {
                bytes.extend(&node.value);
                bytes.extend(b"</");
                bytes.extend(node.name.as_bytes());
                bytes.push(b'>');
            } else if !closing {
                pending.push((id, depth, true));
                pending.extend(node.children.iter().rev().map(|&id| (id, depth + 1, false)));
            }
            bytes.extend(b"\r\n");
        }
        Ok(bytes)
    }

    fn child(&self, parent: usize, name: &str) -> Option<usize> {
        self.nodes[parent]
            .children
            .iter()
            .copied()
            .find(|&id| self.nodes[id].name == name)
    }
}
