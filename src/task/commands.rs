use super::TaskTree;
use crate::{Error, Result, read};
use std::ops::Bound::{Excluded, Unbounded};

/// 任务命令的六个 DWORD；`data[2]` 在直接分派中控制向后代传播。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub target: u32,
    pub sender: u32,
    pub code: u32,
    pub data: [u32; 3],
}

enum Mode {
    Direct,
    Broadcast(Option<u32>),
}

impl TaskTree {
    /// handler 返回 false 时结束当前分支，true 时按 `data[2]` 继续。
    pub fn dispatch(
        &mut self,
        origin: u32,
        command: Command,
        output: &mut u32,
        mut handler: impl FnMut(&mut Self, &Command, &mut u32) -> Result<bool>,
    ) -> Result<bool> {
        self.check_command(origin, command.code)?;
        if self.find_reachable(origin, command.target).is_none() {
            return Ok(false);
        }
        self.traverse(command.target, command, output, Mode::Direct, &mut handler)?;
        Ok(true)
    }

    /// 对应外层命令 11／12；group 为 None 时不筛选对象类型。
    pub fn broadcast(
        &mut self,
        origin: u32,
        group: Option<u32>,
        command: Command,
        output: &mut u32,
        mut handler: impl FnMut(&mut Self, &Command, &mut u32) -> Result<bool>,
    ) -> Result<bool> {
        self.check_command(origin, command.code)?;
        let mut scope = origin;
        let start = loop {
            if let Some(child) = self.first_child_toward(scope, command.target) {
                break child;
            }
            match self.node(scope).unwrap().parent {
                Some(parent) if parent != self.root_id => scope = parent,
                _ => break self.root_id,
            }
        };
        self.traverse(start, command, output, Mode::Broadcast(group), &mut handler)
    }

    fn check_command(&self, origin: u32, code: u32) -> Result<()> {
        if self.node(origin).is_none() {
            return Err(Error::new(0, "命令发送任务不存在"));
        }
        if matches!(code, 1 | 2 | 11 | 12) {
            return Err(Error::new(0, "任务创建、登记或嵌套广播需要专用入口"));
        }
        Ok(())
    }

    fn first_child_toward(&self, scope: u32, target: u32) -> Option<u32> {
        let mut node = self.find_in_subtree(scope, target)?;
        if node.id == scope {
            return None;
        }
        loop {
            let parent = node.parent?;
            if parent == scope {
                return Some(node.id);
            }
            node = self.node(parent)?;
        }
    }

    fn traverse(
        &mut self,
        start: u32,
        mut command: Command,
        output: &mut u32,
        mode: Mode,
        handler: &mut impl FnMut(&mut Self, &Command, &mut u32) -> Result<bool>,
    ) -> Result<bool> {
        let mut parents: Vec<(u32, Option<i32>)> = Vec::new();
        let mut next = Some(start);
        let mut result = true;
        let mut visited = 0;
        while let Some(id) = next {
            visited += 1;
            if visited > read::MAX_ITEMS {
                return Err(Error::new(0, "任务命令遍历超过上限"));
            }
            if let Some(node) = self.node(id) {
                command.target = id;
                let selected = match mode {
                    Mode::Direct | Mode::Broadcast(None) => true,
                    Mode::Broadcast(Some(group)) => node.kind == group,
                };
                if matches!(mode, Mode::Broadcast(_)) {
                    command.data[2] = 0;
                }
                result = !selected || handler(self, &command, output)?;
                if matches!(mode, Mode::Broadcast(_)) || (result && command.data[2] != 0) {
                    parents.push((id, None));
                }
            }
            next = None;
            while let Some((parent, previous)) = parents.last_mut() {
                let child = self.node(*parent).and_then(|node| match previous {
                    Some(previous) => node
                        .children
                        .range((Excluded(*previous), Unbounded))
                        .find(|id| !self.unregistered.contains(id)),
                    None => node
                        .children
                        .iter()
                        .find(|id| !self.unregistered.contains(id)),
                });
                if let Some(&child) = child {
                    *previous = Some(child);
                    next = Some(child as u32);
                    break;
                }
                parents.pop();
            }
        }
        Ok(result)
    }
}
