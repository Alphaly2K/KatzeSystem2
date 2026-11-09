use super::{Command, Control, TaskClock, TaskTree};
use crate::{Error, Result, read};
use std::ops::Bound::{Excluded, Unbounded};

/// 树调度访问的对象状态及同步生命周期操作。
pub trait Lifecycle {
    fn begin_update(&mut self, _id: u32) -> Result<()> {
        Ok(())
    }

    fn state(&mut self, id: u32) -> Result<(&mut Control, &mut TaskClock)>;

    /// 时钟及子任务更新已完成；返回前完成本次主体的预算续跑。
    fn update_body(&mut self, tree: &mut TaskTree, id: u32, increment: u32) -> Result<()>;

    /// 向子任务发送删除请求，返回前完成专属通知及标记。
    fn request_delete(&mut self, tree: &mut TaskTree, command: Command) -> Result<()>;

    /// 子任务已收到请求；按对象类型处理自身通知并标记删除。
    fn mark_destroying(&mut self, tree: &mut TaskTree, id: u32) -> Result<()>;

    /// 已删除子任务的完成通知送达其父级。
    fn notify_child_finished(
        &mut self,
        _tree: &mut TaskTree,
        _parent: u32,
        _id: u32,
    ) -> Result<()> {
        Ok(())
    }

    /// 子树已释放，释放当前对象的资源及执行状态。
    fn prepare_release(&mut self, _tree: &mut TaskTree, _id: u32) -> Result<()> {
        Ok(())
    }

    /// 宿主登记已解除，释放当前对象的资源及执行状态。
    fn release(&mut self, id: u32) -> Result<()>;
}

enum Frame {
    Enter(u32),
    Children(u32, Option<i32>),
}

impl TaskTree {
    /// 完成整棵树的一轮更新；根任务被销毁时返回 false。
    pub fn update(
        &mut self,
        now: u32,
        increment: u32,
        lifecycle: &mut impl Lifecycle,
    ) -> Result<bool> {
        if self.node(self.root_id).is_none() {
            return Err(Error::new(0, "任务树已经关闭"));
        }
        self.update_scope(self.root_id, now, increment, lifecycle, false)?;
        Ok(self.node(self.root_id).is_some())
    }

    /// 显式主体调用前推进子任务；所属对象由调用者持有并更新时钟。
    pub(crate) fn update_children(
        &mut self,
        owner: u32,
        now: u32,
        increment: u32,
        lifecycle: &mut impl Lifecycle,
    ) -> Result<()> {
        if self.node(owner).is_none() {
            return Err(Error::new(0, "任务不存在"));
        }
        self.update_scope(owner, now, increment, lifecycle, true)
    }

    fn update_scope(
        &mut self,
        scope: u32,
        now: u32,
        increment: u32,
        lifecycle: &mut impl Lifecycle,
        children_only: bool,
    ) -> Result<()> {
        let mut frames = vec![if children_only {
            Frame::Children(scope, None)
        } else {
            Frame::Enter(scope)
        }];
        let mut visited = 0;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Enter(id) => {
                    if self.node(id).is_none() {
                        continue;
                    }
                    count_visit(&mut visited)?;
                    lifecycle.begin_update(id)?;
                    lifecycle.state(id)?.1.update(now);
                    frames.push(Frame::Children(id, None));
                }
                Frame::Children(id, previous) => {
                    if self.node(id).is_none() {
                        continue;
                    }
                    if let Some(child) = self.next_child(id, previous) {
                        frames.push(Frame::Children(id, Some(child as i32)));
                        frames.push(Frame::Enter(child));
                        continue;
                    }
                    if children_only && id == scope {
                        continue;
                    }
                    if id == self.root_id {
                        self.sweep();
                    }
                    if lifecycle.state(id)?.0.deletion_requested != 0 {
                        self.destroy_inner(id, lifecycle, &mut visited)?;
                    } else {
                        lifecycle.update_body(self, id, increment)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 同步发送子任务删除通知并释放子树；也可用于关闭根任务。
    pub fn destroy(&mut self, id: u32, lifecycle: &mut impl Lifecycle) -> Result<()> {
        if self.node(id).is_none() {
            return Err(Error::new(0, "任务不存在"));
        }
        self.destroy_inner(id, lifecycle, &mut 0)
    }

    fn destroy_inner(
        &mut self,
        id: u32,
        lifecycle: &mut impl Lifecycle,
        visited: &mut usize,
    ) -> Result<()> {
        let mut frames = vec![Frame::Enter(id)];
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Enter(id) => {
                    if self.node(id).is_none() {
                        continue;
                    }
                    count_visit(visited)?;
                    if id != self.root_id {
                        self.unregister(id)?;
                    }
                    let mut previous = None;
                    while let Some(child) = self.next_child(id, previous) {
                        count_visit(visited)?;
                        previous = Some(child as i32);
                        lifecycle.request_delete(
                            self,
                            Command {
                                target: child,
                                sender: id,
                                code: 3,
                                data: [0; 3],
                            },
                        )?;
                    }
                    if self.node(id).is_some() {
                        lifecycle.mark_destroying(self, id)?;
                        frames.push(Frame::Children(id, None));
                    }
                }
                Frame::Children(id, previous) => {
                    if self.node(id).is_none() {
                        continue;
                    }
                    if let Some(child) = self.next_child(id, previous) {
                        frames.push(Frame::Children(id, Some(child as i32)));
                        frames.push(Frame::Enter(child));
                        continue;
                    }
                    lifecycle.prepare_release(self, id)?;
                    lifecycle.release(id)?;
                    if let Some(node) = self.nodes.remove(&(id as i32)) {
                        let parent = node.parent;
                        if let Some(parent) = parent.and_then(|id| self.nodes.get_mut(&(id as i32)))
                        {
                            parent.children.remove(&(id as i32));
                        }
                        self.unregistered.remove(&(id as i32));
                        if let Some(parent) = parent {
                            lifecycle.notify_child_finished(self, parent, id)?;
                        }
                    }
                    if id == self.root_id {
                        self.reserved.clear();
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn next_child(&self, id: u32, previous: Option<i32>) -> Option<u32> {
        let children = &self.node(id)?.children;
        let child = match previous {
            Some(previous) => children
                .range((Excluded(previous), Unbounded))
                .find(|id| !self.unregistered.contains(id)),
            None => children.iter().find(|id| !self.unregistered.contains(id)),
        };
        child.map(|&id| id as u32)
    }
}

fn count_visit(visited: &mut usize) -> Result<()> {
    *visited += 1;
    if *visited > read::MAX_ITEMS {
        return Err(Error::new(0, "任务生命周期遍历超过上限"));
    }
    Ok(())
}
