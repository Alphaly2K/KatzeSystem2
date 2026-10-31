use crate::{Error, Result, read};
use std::collections::BTreeMap;

#[derive(Default)]
struct Node {
    children: BTreeMap<i8, Node>,
}

#[derive(Default)]
pub(super) struct SequenceMap {
    names: BTreeMap<Vec<u8>, Node>,
    nodes: usize,
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
            node = node.children.entry(value).or_insert_with(|| {
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
            let Some(child) = node.children.get(value) else {
                return Ok(false);
            };
            node = child;
        }
        Ok(node.children.is_empty() || node.children.contains_key(&-1))
    }
}
