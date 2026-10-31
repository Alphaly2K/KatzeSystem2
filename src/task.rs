use crate::{Error, Result, read};
use std::collections::{BTreeMap, BTreeSet};

mod commands;
pub use commands::Command;

/// 任务树中的编号、父级与类型；任务执行状态由对应解释器保存。
#[derive(Clone, Debug)]
pub struct TaskNode {
    pub id: u32,
    pub parent: Option<u32>,
    pub kind: u32,
    children: BTreeSet<i32>,
}

/// 维护任务编号、父子索引与对象命令路由。
#[derive(Clone, Debug)]
pub struct TaskTree {
    root_id: u32,
    nodes: BTreeMap<i32, TaskNode>,
    reserved: BTreeSet<i32>,
}

impl TaskTree {
    pub fn new(root_id: u32, kind: u32) -> Self {
        Self {
            root_id,
            reserved: BTreeSet::from([root_id as i32]),
            nodes: BTreeMap::from([(
                root_id as i32,
                TaskNode {
                    id: root_id,
                    parent: None,
                    kind,
                    children: BTreeSet::new(),
                },
            )]),
        }
    }

    pub fn root_id(&self) -> u32 {
        self.root_id
    }

    pub fn node(&self, id: u32) -> Option<&TaskNode> {
        self.nodes.get(&(id as i32))
    }

    pub fn children(&self, id: u32) -> Option<impl DoubleEndedIterator<Item = u32> + '_> {
        Some(self.node(id)?.children.iter().map(|&id| id as u32))
    }

    pub fn descendants(&self, id: u32) -> Option<impl Iterator<Item = u32> + '_> {
        self.node(id)?;
        Some(self.nodes.values().filter_map(move |node| {
            (node.id != id && self.find_in_subtree(id, node.id).is_some()).then_some(node.id)
        }))
    }

    pub fn find_in_subtree(&self, scope: u32, id: u32) -> Option<&TaskNode> {
        self.node(scope)?;
        let target = self.node(id)?;
        let mut node = target;
        loop {
            if node.id == scope {
                return Some(target);
            }
            node = self.node(node.parent?)?;
        }
    }

    pub fn next_id(&self) -> u32 {
        let mut next = 0;
        for &id in self.reserved.range(0..) {
            if id as u32 != next {
                break;
            }
            next += 1;
        }
        next
    }

    pub fn insert_auto(&mut self, parent: u32, kind: u32) -> Result<u32> {
        let id = self.next_id();
        self.insert(parent, id, kind)?;
        Ok(id)
    }

    pub fn insert(&mut self, parent: u32, id: u32, kind: u32) -> Result<()> {
        if self.node(parent).is_none() {
            return Err(Error::new(0, "任务父级不存在"));
        }
        if self.node(id).is_some() {
            return Err(Error::new(0, "任务编号已登记"));
        }
        if self.reserved.len() >= read::MAX_ITEMS && !self.reserved.contains(&(id as i32)) {
            return Err(Error::new(0, "任务数量超过上限"));
        }
        self.nodes.insert(
            id as i32,
            TaskNode {
                id,
                parent: Some(parent),
                kind,
                children: BTreeSet::new(),
            },
        );
        self.reserved.insert(id as i32);
        self.nodes
            .get_mut(&(parent as i32))
            .unwrap()
            .children
            .insert(id as i32);
        Ok(())
    }

    /// 返回父任务优先、同级按有符号编号升序排列的子树编号。
    pub fn subtree_ids(&self, id: u32) -> Result<Vec<u32>> {
        if self.node(id).is_none() {
            return Err(Error::new(0, "任务不存在"));
        }
        let mut pending = vec![id];
        let mut ids = Vec::new();
        while let Some(id) = pending.pop() {
            ids.push(id);
            pending.extend(self.children(id).unwrap().rev());
        }
        Ok(ids)
    }

    /// 在对象销毁时清除整棵子树，返回被移除的编号。
    pub fn remove(&mut self, id: u32) -> Result<Vec<u32>> {
        if id == self.root_id {
            return Err(Error::new(0, "不能单独移除任务树根节点"));
        }
        let ids = self.subtree_ids(id)?;
        let parent = self.node(id).unwrap().parent.unwrap();
        self.nodes
            .get_mut(&(parent as i32))
            .unwrap()
            .children
            .remove(&(id as i32));
        for id in &ids {
            self.nodes.remove(&(*id as i32));
        }
        Ok(ids)
    }

    /// 在根任务完成索引清理后释放已销毁任务的编号。
    pub fn sweep(&mut self) {
        self.reserved.retain(|id| self.nodes.contains_key(id));
    }
}
