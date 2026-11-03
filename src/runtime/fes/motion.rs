use super::{ObjectState, Objects};
use crate::kcs::{MotionHost, MotionState, MotionTask, Program};
use crate::{Error, Result};

pub(super) struct Traversal {
    root: u32,
    stopping: bool,
    now: u32,
    order: Vec<u32>,
    cursor: usize,
}

impl Objects {
    pub(super) fn require_motion_idle(&self) -> Result<()> {
        if self.motion_update.is_some() {
            return Err(Error::new(0, "FES 对象运动更新尚未完成"));
        }
        Ok(())
    }

    /// 查询目标及其后代是否仍有活动运动，独立于最近一次媒体刷新。
    pub fn motion_active(&self, handle: u32) -> Result<bool> {
        let mut stack = vec![handle];
        while let Some(handle) = stack.pop() {
            let object = self
                .get(handle)
                .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
            if object.motions.any_running(None)? {
                return Ok(true);
            }
            stack.extend(object.children());
        }
        Ok(false)
    }

    /// 仅向目标及后代请求结束；队列在后续正常更新中执行，不清空或重设延迟。
    pub fn request_motion_finish(&mut self, handle: u32) -> Result<()> {
        self.require_motion_idle()?;
        let mut stack = vec![handle];
        while let Some(handle) = stack.pop() {
            let object = self
                .get_mut(handle)
                .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
            stack.extend(object.children());
            object.motions.request_finish_group(None)?;
        }
        Ok(())
    }

    /// 向对象加入分组零的运动；delay 为运动入口开始前的时间偏移。
    pub fn start_motion(
        &mut self,
        handle: u32,
        program: Program,
        parameters: &[f32],
        delay: u32,
    ) -> Result<u32> {
        let state = MotionState::with_parameters(parameters)?;
        self.capture_motion_base(handle)?;
        self.insert_motion(handle, program, state, delay)
    }

    pub(super) fn capture_motion_base(&mut self, handle: u32) -> Result<()> {
        self.require_motion_idle()?;
        let object = self
            .get(handle)
            .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
        if object.state.image().is_none()
            && !matches!(object.state, ObjectState::Base)
            && !matches!(object.state, ObjectState::Draw(_))
            && object.state.control().is_none()
            && !matches!(object.state, ObjectState::Sound(_))
            && object.state.plane().is_none()
        {
            return Err(Error::new(0, "FES 此对象类型的运动属性尚未接入"));
        }
        let capture = !self.motion_active(handle)?;
        let object = self.get_mut(handle).unwrap();
        if capture {
            if let Some(image) = object.state.image() {
                object.motions.base_properties = image.motion_properties();
            } else if let Some(slider) = object.state.control() {
                object.motions.base_properties = slider.motion_properties();
            } else if let ObjectState::Sound(sound) = &object.state {
                object.motions.base_properties = sound.motion_properties();
            } else if let Some(plane) = object.state.plane() {
                object.motions.base_properties = plane.motion_properties();
            }
        }
        Ok(())
    }

    pub(super) fn insert_motion(
        &mut self,
        handle: u32,
        program: Program,
        state: MotionState,
        delay: u32,
    ) -> Result<u32> {
        self.get_mut(handle)
            .unwrap()
            .motions
            .insert(MotionTask::new(program, state, delay)?, 0)
    }

    /// 先子后父推进根对象子树；false 后以同一宿主续跑，期间不修改对象树。
    pub fn update_motions(
        &mut self,
        now: u32,
        host: &mut MotionHost,
        budget_per_task: usize,
    ) -> Result<bool> {
        self.advance_motions(self.top, now, false, host, budget_per_task)
    }

    /// 请求后代结束并以零时刻更新子树，最后清空目标自身队列和延迟。
    pub fn stop_motions(
        &mut self,
        handle: u32,
        host: &mut MotionHost,
        budget_per_task: usize,
    ) -> Result<bool> {
        self.advance_motions(handle, 0, true, host, budget_per_task)
    }

    fn advance_motions(
        &mut self,
        root: u32,
        now: u32,
        stopping: bool,
        host: &mut MotionHost,
        budget_per_task: usize,
    ) -> Result<bool> {
        if self
            .motion_update
            .as_ref()
            .is_some_and(|pending| pending.root != root || pending.stopping != stopping)
        {
            return Err(Error::new(0, "FES 另一次对象运动更新尚未完成"));
        }
        if self.motion_update.is_none() {
            let mut order = Vec::new();
            let mut stack = vec![root];
            while let Some(handle) = stack.pop() {
                order.push(handle);
                stack.extend(
                    self.get(handle)
                        .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?
                        .children(),
                );
            }
            order.reverse();
            if stopping {
                for &handle in &order {
                    self.get_mut(handle)
                        .unwrap()
                        .motions
                        .request_finish_group(None)?;
                }
            }
            self.motion_update = Some(Traversal {
                root,
                stopping,
                now,
                order,
                cursor: 0,
            });
        }
        loop {
            let traversal = self.motion_update.as_ref().unwrap();
            let Some(&handle) = traversal.order.get(traversal.cursor) else {
                if stopping {
                    let object = self.get_mut(root).unwrap();
                    object.motions.clear();
                    object.motion_delay = 0;
                }
                self.motion_update = None;
                return Ok(true);
            };
            let object = self.objects.get_mut(&handle).unwrap();
            if !object
                .motions
                .update_object(traversal.now, host, budget_per_task.max(1))?
            {
                return Ok(false);
            }
            if object.motions.was_updated() {
                if let Some(image) = object.state.image_mut() {
                    if image.apply_motion_properties(object.motions.properties()) {
                        self.invalidate(handle)?;
                    }
                } else if let Some(slider) = object.state.control_mut() {
                    slider.apply_motion_properties(object.motions.properties());
                } else if let ObjectState::Sound(sound) = &mut object.state {
                    if sound.apply_motion_properties(object.motions.properties()) {
                        self.invalidate(handle)?;
                    }
                } else if let Some(plane) = object.state.plane_mut() {
                    if plane.apply_motion_properties(object.motions.properties()) {
                        self.invalidate(handle)?;
                    }
                }
            }
            self.motion_update.as_mut().unwrap().cursor += 1;
        }
    }
}
