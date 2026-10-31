use super::{Command, TaskClock};
use crate::{Error, Result};

/// 任务基类的删除标记与命令 23／24 使用的原始属性。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub deletion_requested: i32,
    pub property: u32,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            deletion_requested: 0,
            property: u32::MAX,
        }
    }
}

impl Control {
    /// 对象处理器返回 true 后执行基类命令；false 表示当前分支被消费。
    pub fn dispatch(
        &mut self,
        kind: u32,
        clock: &mut TaskClock,
        command: &Command,
        output: &mut u32,
        specific: impl FnOnce(&mut Self, &mut TaskClock, &Command, &mut u32) -> Result<bool>,
    ) -> Result<bool> {
        if self.deletion_requested != 0 || !specific(self, clock, command, output)? {
            return Ok(false);
        }
        match command.code {
            3 => self.deletion_requested = 1,
            13 => *output = kind,
            17 => {}
            23 => *output = self.property,
            24 => self.property = command.data[1],
            29 => clock.paused = command.data[1] as i32,
            2 | 15 | 16 => {
                return Err(Error::new(0, "任务登记与序列化命令需要专用处理器"));
            }
            _ => return Ok(true),
        }
        Ok(false)
    }
}
