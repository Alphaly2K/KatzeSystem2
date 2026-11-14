use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 序列集合装载的文件数据；已识别的 CGL 头被移除，正文保持原始字节。
pub struct SequenceData {
    bytes: Vec<u8>,
    stride: u32,
}

impl Default for SequenceData {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            stride: 256,
        }
    }
}

impl SequenceData {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 每条序列字符串记录的字节跨度。
    pub fn record_stride(&self) -> u32 {
        self.stride
    }

    fn group(&self, id: u32, name: &[u8]) -> Result<Option<(usize, u32)>> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let name = name.split(|&byte| byte == 0).next().unwrap();
        let mut group = 0;
        for _ in 0..read::MAX_ITEMS {
            let header = read::range(&self.bytes, group, 12)?;
            if read::u32_at(header, 4)? == id {
                let count = read::u32_at(header, 8)? as usize;
                if count > read::MAX_ITEMS {
                    return Err(Error::new(group, "KCS 序列文件分组数量超过上限"));
                }
                let mut entry = group + 12;
                for _ in 0..count {
                    let header = read::range(&self.bytes, entry, 72)?;
                    if read::zstr(&self.bytes, entry + 4)?.eq_ignore_ascii_case(name) {
                        return Ok(Some((entry + 72, read::u32_at(header, 68)?)));
                    }
                    let next = read::u32_at(header, 0)?;
                    if next == 0 {
                        break;
                    }
                    entry = advance(entry, next)?;
                }
            }
            let next = read::u32_at(header, 0)?;
            if next == 0 {
                return Ok(None);
            }
            group = advance(group, next)?;
        }
        Err(Error::new(group, "KCS 序列文件分组链超过上限"))
    }
}

fn advance(offset: usize, increment: u32) -> Result<usize> {
    offset
        .checked_add(increment as usize)
        .ok_or_else(|| Error::new(offset, "KCS 序列文件偏移溢出"))
}

#[derive(Default)]
struct Node {
    children: BTreeMap<i32, Node>,
}

#[derive(Default)]
pub(super) struct SequenceMap {
    names: BTreeMap<Vec<u8>, Node>,
    nodes: usize,
    pub(super) data: SequenceData,
}

fn parse(source: &[u8]) -> Result<Option<(Vec<u8>, Vec<i8>)>> {
    if source.len() > 254 {
        return Err(Error::new(0, "KCS 序列文本超过缓冲区上限"));
    }
    let table = super::strings::split(source, b",");
    let mut parts = table.split(|&byte| byte == 0).filter(|s| !s.is_empty());
    let Some(first) = parts.next() else {
        return Ok(None);
    };
    let mut name = first.to_vec();
    if let Some(second) = parts.next() {
        name.push(b'_');
        name.extend_from_slice(second);
    }
    let values: Vec<_> = parts.map(|part| part[0] as i8).collect();
    if values.len() > 64 {
        return Err(Error::new(0, "KCS 序列分量超过缓冲区上限"));
    }
    Ok(Some((name, values)))
}

impl SequenceMap {
    pub fn import_group(&mut self, id: u32, name: &[u8]) -> Result<()> {
        let Some((mut offset, count)) = self.data.group(id, name)? else {
            return Ok(());
        };
        if count as usize > read::MAX_ITEMS {
            return Err(Error::new(offset, "KCS 序列文件记录数量超过上限"));
        }
        for _ in 0..count {
            if let Some((name, values)) = parse(read::zstr(&self.data.bytes, offset)?)? {
                self.insert(&name, &values)?;
            }
            offset = advance(offset, self.data.stride)?;
        }
        Ok(())
    }

    pub fn load(&mut self, mut bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "KCS 序列文件超过读取上限"));
        }
        let stride = if bytes.len() > 4 && bytes.starts_with(b"CGL\0") {
            bytes.drain(..4);
            256
        } else {
            64
        };
        self.data = SequenceData { bytes, stride };
        Ok(())
    }

    pub fn clear(&mut self) {
        self.names.clear();
        self.nodes = 0;
    }

    pub fn register(&mut self, source: &[u8], name_mode: u32) -> Result<()> {
        let Some((name, values)) = parse(source)? else {
            return Ok(());
        };
        self.insert(&name, &values)?;
        if matches!(name_mode, 0 | 1) {
            if let Some(index) = (2..name.len())
                .find(|&index| name[index] == b'_' && matches!(name[index - 1], b'l' | b'L'))
            {
                let mut variant = name;
                if name_mode == 0 {
                    variant.remove(index - 1);
                } else {
                    variant[index - 1] = b'm';
                }
                self.insert(&variant, &values)?;
            }
        }
        Ok(())
    }

    fn insert(&mut self, name: &[u8], values: &[i8]) -> Result<()> {
        if self.nodes > read::MAX_ITEMS.saturating_sub(values.len() + 2) {
            return Err(Error::new(0, "KCS 序列节点数量超过上限"));
        }
        let mut node = self.names.entry(name.to_vec()).or_insert_with(|| {
            self.nodes += 1;
            Node::default()
        });
        // −1 同时是原树的结束边和合法的有符号字节。
        for value in values.iter().copied().chain(Some(-1)) {
            node = node.children.entry(i32::from(value)).or_insert_with(|| {
                self.nodes += 1;
                Node::default()
            });
        }
        Ok(())
    }

    pub fn contains(&self, source: &[u8]) -> Result<bool> {
        let Some((name, values)) = parse(source)? else {
            return Ok(false);
        };
        let Some(mut node) = self.names.get(&name) else {
            return Ok(false);
        };
        for (index, value) in values.iter().enumerate() {
            if node.children.is_empty() {
                return Ok(values[index..].iter().all(|&value| value == 0));
            }
            let Some(child) = node.children.get(&i32::from(*value)) else {
                return Ok(false);
            };
            node = child;
        }
        Ok(node.children.is_empty() || node.children.contains_key(&-1))
    }

    fn save(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for (name, node) in &self.names {
            if name.len() >= 64 {
                return Err(Error::new(bytes.len(), "KCS 序列存档名称超过固定字段"));
            }
            let offset = extend(&mut bytes, 68)?;
            bytes[offset..offset + 4].copy_from_slice(&(node.children.len() as u32).to_le_bytes());
            bytes[offset + 4..offset + 4 + name.len()].copy_from_slice(name);
            node.save_children(&mut bytes)?;
        }
        Ok(bytes)
    }

    fn restore(&mut self, bytes: &[u8]) -> Result<()> {
        self.clear();
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "KCS 序列存档超过读取上限"));
        }
        let mut offset = 0;
        while offset < bytes.len() {
            let header = read::range(bytes, offset, 68)?;
            let count = read::u32_at(header, 0)? as i32;
            let name = read::zstr(&header[4..], 0)?.to_vec();
            // 原导入遇到重复名称即结束，包装入口仍返回成功。
            if self.names.contains_key(&name) {
                break;
            }
            add_node(&mut self.nodes, offset)?;
            let node = self.names.entry(name).or_default();
            offset += 68;
            node.restore_children(bytes, &mut offset, count, 0, &mut self.nodes)?;
        }
        Ok(())
    }
}

fn extend(bytes: &mut Vec<u8>, length: usize) -> Result<usize> {
    let start = bytes.len();
    let end = start
        .checked_add(length)
        .filter(|&n| n <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(start, "KCS 序列存档超过存储上限"))?;
    bytes.resize(end, 0);
    Ok(start)
}

fn add_node(nodes: &mut usize, offset: usize) -> Result<()> {
    if *nodes >= read::MAX_ITEMS {
        return Err(Error::new(offset, "KCS 序列节点数量超过上限"));
    }
    *nodes += 1;
    Ok(())
}

impl Node {
    fn save_children(&self, bytes: &mut Vec<u8>) -> Result<()> {
        for (key, node) in &self.children {
            let offset = extend(bytes, 8)?;
            bytes[offset..offset + 4].copy_from_slice(&(node.children.len() as u32).to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&key.to_le_bytes());
            node.save_children(bytes)?;
        }
        Ok(())
    }

    fn restore_children(
        &mut self,
        bytes: &[u8],
        offset: &mut usize,
        count: i32,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<()> {
        if count <= 0 {
            return Ok(());
        }
        if depth >= 256 || count as usize > read::MAX_ITEMS {
            return Err(Error::new(*offset, "KCS 序列存档层级或节点数量超过上限"));
        }
        for _ in 0..count {
            let header = read::range(bytes, *offset, 8)?;
            let children = read::u32_at(header, 0)? as i32;
            let key = read::u32_at(header, 4)? as i32;
            if self.children.contains_key(&key) {
                return Err(Error::new(*offset, "KCS 序列存档包含重复边"));
            }
            add_node(nodes, *offset)?;
            *offset += 8;
            self.children.entry(key).or_default().restore_children(
                bytes,
                offset,
                children,
                depth + 1,
                nodes,
            )?;
        }
        Ok(())
    }
}

pub(super) fn call(
    host: &mut super::HostState,
    index: u32,
    arguments: &[u8],
    vm: &super::Interpreter,
) -> Result<super::NativeReturn> {
    let container = read::u32_at(arguments, 0)?;
    if arguments.len() != 12 && !(container == 0 && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if container == 0 {
        return Ok(super::NativeReturn::Value(0));
    }
    let record = read::u32_at(arguments, 4)?;
    let handle = read::u32_at(arguments, 8)?;
    if index == 424 {
        if handle == 0 {
            return Ok(super::NativeReturn::Value(0));
        }
        let bytes = host
            .sequence_maps
            .get(&handle)
            .ok_or_else(|| Error::new(vm.pc, "KCS 序列对象句柄无效"))?
            .save()?;
        host.data_container_mut(container)?.append(record, &bytes)?;
    } else {
        let Some(bytes) = host.data_container(container)?.record(record)? else {
            return Ok(super::NativeReturn::Value(0));
        };
        if handle == 0 {
            return Ok(super::NativeReturn::Value(0));
        }
        let bytes = bytes.to_vec();
        host.sequence_maps
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "KCS 序列对象句柄无效"))?
            .restore(&bytes)?;
    }
    Ok(super::NativeReturn::Value(1))
}
