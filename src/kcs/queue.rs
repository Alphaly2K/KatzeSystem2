use super::{MotionHost, MotionTask, numeric};
use crate::{Error, Result, Step, read};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

struct Entry {
    task: MotionTask,
    group: i32,
    origin: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Update,
    Finish,
}

struct Batch {
    operation: Operation,
    now: u32,
    skip_unstarted: bool,
    speed: f32,
    cursor: Option<u32>,
    resuming: bool,
}

/// 按创建顺序组合运动属性；运行标志的查询与队列删除分别进行。
pub struct MotionQueue {
    pub base_properties: [u32; 36],
    pub speed: f32,
    entries: BTreeMap<u32, Entry>,
    next_id: u32,
    properties: [u32; 36],
    batch: Option<Batch>,
    updated: bool,
}

impl std::fmt::Debug for MotionQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MotionQueue")
            .field("len", &self.len())
            .field("speed", &self.speed)
            .field("pending", &self.is_pending())
            .finish_non_exhaustive()
    }
}

impl Default for MotionQueue {
    fn default() -> Self {
        let mut properties = [0; 36];
        for i in [9, 14, 19, 24, 25, 26, 27] {
            properties[i] = 1.0f32.to_bits();
        }
        properties[28] = u32::MAX;
        properties[35] = 1;
        Self {
            base_properties: properties,
            speed: 1.0,
            entries: BTreeMap::new(),
            next_id: 1,
            properties,
            batch: None,
            updated: false,
        }
    }
}

impl MotionQueue {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_pending(&self) -> bool {
        self.batch.is_some()
    }

    pub fn was_updated(&self) -> bool {
        self.updated
    }

    pub fn properties(&self) -> &[u32; 36] {
        &self.properties
    }

    pub fn set_properties(&mut self, properties: [u32; 36]) -> Result<()> {
        self.require_idle()?;
        self.properties = properties;
        Ok(())
    }

    /// 替换指定任务的参数，余下参数清零；不改变执行状态。
    pub fn set_parameters(&mut self, id: u32, parameters: &[u32]) -> Result<bool> {
        self.require_idle()?;
        if parameters.len() > 256 {
            return Err(Error::new(0, "运动参数超过上限"));
        }
        let Some(entry) = self.entries.get_mut(&id) else {
            return Ok(false);
        };
        let mut bytes = [0; 1024];
        for (target, value) in bytes.chunks_exact_mut(4).zip(parameters) {
            target.copy_from_slice(&value.to_le_bytes());
        }
        entry.task.interpreter.write(0x8000_009c, &bytes)?;
        Ok(true)
    }

    pub fn task(&self, id: u32) -> Option<&MotionTask> {
        self.entries.get(&id).map(|entry| &entry.task)
    }

    pub fn group(&self, id: u32) -> Option<i32> {
        self.entries.get(&id).map(|entry| entry.group)
    }

    pub fn time_origin(&self, id: u32) -> Option<u32> {
        self.entries.get(&id).map(|entry| entry.origin)
    }

    pub fn insert(&mut self, task: MotionTask, group: i32) -> Result<u32> {
        self.require_idle()?;
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "运动队列超过上限"));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "运动任务编号耗尽"))?;
        self.entries.insert(
            id,
            Entry {
                task,
                group,
                origin: 0,
            },
        );
        Ok(id)
    }

    pub fn remove(&mut self, id: u32) -> Result<bool> {
        self.require_idle()?;
        Ok(self.entries.remove(&id).is_some())
    }

    pub fn remove_group(&mut self, group: i32) -> Result<usize> {
        self.require_idle()?;
        let before = self.len();
        self.entries.retain(|_, entry| entry.group != group);
        Ok(before - self.len())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.batch = None;
    }

    pub fn any_running(&self, group: Option<i32>) -> Result<bool> {
        for entry in self.entries.values() {
            if group.is_none_or(|group| group == entry.group) && entry.task.state()?.running != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn set_time_origin(&mut self, id: Option<u32>, origin: u32) -> Result<usize> {
        self.require_idle()?;
        let mut count = 0;
        for (&key, entry) in &mut self.entries {
            if id.is_none_or(|id| id == key) {
                entry.origin = origin;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn request_finish(&mut self, id: u32) -> Result<bool> {
        self.require_idle()?;
        if let Some(entry) = self.entries.get_mut(&id) {
            entry
                .task
                .interpreter
                .write(0x8000_0098, &1u32.to_le_bytes())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn request_finish_group(&mut self, group: Option<i32>) -> Result<usize> {
        self.require_idle()?;
        let mut count = 0;
        for entry in self.entries.values_mut() {
            if group.is_none_or(|group| group == entry.group) {
                entry
                    .task
                    .interpreter
                    .write(0x8000_0098, &1u32.to_le_bytes())?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// true 为本次遍历完成，false 为预算让出；预算按每个任务分别提供。
    pub fn update(
        &mut self,
        now: u32,
        skip_unstarted: bool,
        host: &mut MotionHost,
        budget_per_task: usize,
    ) -> Result<bool> {
        if self
            .batch
            .as_ref()
            .is_some_and(|batch| batch.operation == Operation::Finish)
        {
            return Err(Error::new(0, "运动队列的结束调用尚未完成"));
        }
        if self.batch.is_none() {
            self.begin(Operation::Update, now, skip_unstarted);
        }
        self.run(host, budget_per_task)
    }

    /// 对象更新只推进仍有活动任务的队列，并在全部停止后统一删除。
    pub fn update_object(
        &mut self,
        now: u32,
        host: &mut MotionHost,
        budget_per_task: usize,
    ) -> Result<bool> {
        if self.batch.is_none() && !self.any_running(None)? {
            self.updated = false;
            return Ok(true);
        }
        let complete = self.update(now, false, host, budget_per_task)?;
        if complete && !self.any_running(None)? {
            self.clear();
        }
        Ok(complete)
    }

    pub fn finish(&mut self, host: &mut MotionHost, budget_per_task: usize) -> Result<bool> {
        if self
            .batch
            .as_ref()
            .is_none_or(|batch| batch.operation != Operation::Finish)
        {
            self.begin(Operation::Finish, 0, false);
        }
        self.run(host, budget_per_task)
    }

    fn require_idle(&self) -> Result<()> {
        if self.batch.is_some() {
            Err(Error::new(0, "运动队列尚有预算续跑"))
        } else {
            Ok(())
        }
    }

    fn begin(&mut self, operation: Operation, now: u32, skip_unstarted: bool) {
        self.properties = self.base_properties;
        self.updated = false;
        self.batch = Some(Batch {
            operation,
            now,
            skip_unstarted,
            speed: self.speed,
            cursor: None,
            resuming: false,
        });
    }

    fn run(&mut self, host: &mut MotionHost, budget_per_task: usize) -> Result<bool> {
        loop {
            let batch = self.batch.as_mut().unwrap();
            let id = if batch.resuming {
                batch.cursor
            } else if let Some(cursor) = batch.cursor {
                self.entries
                    .range((Excluded(cursor), Unbounded))
                    .next()
                    .map(|(&id, _)| id)
            } else {
                self.entries.keys().next().copied()
            };
            let Some(id) = id else {
                self.batch = None;
                return Ok(true);
            };
            let entry = self.entries.get_mut(&id).unwrap();
            batch.cursor = Some(id);
            if !batch.resuming {
                if batch.operation == Operation::Update
                    && batch.now != u32::MAX
                    && entry.origin == 0
                {
                    if batch.skip_unstarted {
                        continue;
                    }
                    entry.origin = batch.now;
                }
                entry.task.set_properties(&self.properties)?;
            }
            let result = if batch.operation == Operation::Finish {
                entry.task.finish(host, budget_per_task)
            } else {
                let time = (batch.now.wrapping_sub(entry.origin) as f32 * batch.speed) + 0.5;
                let time = numeric::convert(0x38, time.to_bits());
                entry
                    .task
                    .update(time, host, budget_per_task)
                    .map(|step| step.unwrap_or(Step::Halted))
            };
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    self.batch = None;
                    return Err(error);
                }
            };
            if result == Step::Yield {
                batch.resuming = true;
                return Ok(false);
            }
            batch.resuming = false;
            self.properties = entry.task.state()?.properties;
            self.updated = true;
        }
    }
}
