use crate::{Error, Result, read};

const MAX_DEPTH: usize = 256;
const MAX_NODES: usize = 65_536;

/// 状态树节点；子节点顺序对应原对象的兄弟链顺序。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateNode {
    pub data: Vec<u8>,
    pub children: Vec<StateNode>,
}

/// 普通 688／689 的状态树；不包含原容器的虚拟根节点。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateTree {
    pub roots: Vec<StateNode>,
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}

impl Budget {
    fn node(&mut self, depth: usize, length: usize) -> Result<()> {
        if depth >= MAX_DEPTH || self.nodes == MAX_NODES {
            return Err(Error::new(0, "状态树深度或节点数超过上限"));
        }
        self.bytes = self
            .bytes
            .checked_add(12)
            .and_then(|size| size.checked_add(length))
            .filter(|&size| size <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(0, "状态树正文超过读取上限"))?;
        self.nodes += 1;
        Ok(())
    }
}

impl StateTree {
    /// 读取相对偏移记录；原解码器将每个新节点插入链首，各层兄弟顺序因此反转。
    /// 记录必须至少含一个节点，尾部未引用字节忽略。
    pub fn from_record(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "状态树记录超过读取上限"));
        }
        Ok(Self {
            roots: decode_chain(bytes, 0, 0, &mut Budget::default())?,
        })
    }

    /// 按当前兄弟链顺序深度优先写出记录；空树返回空字节。
    /// 每个节点为兄弟偏移、子偏移、正文长度及紧随其后的正文，无额外对齐。
    pub fn to_record(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        encode_chain(&self.roots, 0, &mut Budget::default(), &mut bytes)?;
        Ok(bytes)
    }
}

fn decode_chain(
    bytes: &[u8],
    mut at: usize,
    depth: usize,
    budget: &mut Budget,
) -> Result<Vec<StateNode>> {
    let mut nodes = Vec::new();
    loop {
        let header = read::range(bytes, at, 12)?;
        let next = read::u32_at(header, 0)? as usize;
        let child = read::u32_at(header, 4)? as usize;
        let length = read::u32_at(header, 8)? as usize;
        budget.node(depth, length)?;
        let data = read::range(bytes, at + 12, length)?.to_vec();
        let link = |relative| {
            at.checked_add(relative)
                .filter(|&target| target < bytes.len())
                .ok_or_else(|| Error::new(at, "状态树节点链接越界"))
        };
        let children = if child == 0 {
            Vec::new()
        } else {
            decode_chain(bytes, link(child)?, depth + 1, budget)?
        };
        nodes.push(StateNode { data, children });
        if next == 0 {
            break;
        }
        at = link(next)?;
    }
    nodes.reverse();
    Ok(nodes)
}

fn encode_chain(
    nodes: &[StateNode],
    depth: usize,
    budget: &mut Budget,
    bytes: &mut Vec<u8>,
) -> Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        budget.node(depth, node.data.len())?;
        let at = bytes.len();
        bytes.resize(at + 12, 0);
        bytes[at + 8..at + 12].copy_from_slice(&(node.data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&node.data);
        if !node.children.is_empty() {
            let child = (bytes.len() - at) as u32;
            bytes[at + 4..at + 8].copy_from_slice(&child.to_le_bytes());
            encode_chain(&node.children, depth + 1, budget, bytes)?;
        }
        if index + 1 < nodes.len() {
            let next = (bytes.len() - at) as u32;
            bytes[at..at + 4].copy_from_slice(&next.to_le_bytes());
        }
    }
    Ok(())
}
