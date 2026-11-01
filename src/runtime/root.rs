use super::{BaseObject, TaskObject};
use crate::kcs::{Host, Interpreter};
use crate::task::{Command, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 保存输入与声音任务编号的根对象；子任务由调用者创建并登记。
pub struct RootObject {
    base: BaseObject,
    input_task: u32,
    sound_task: u32,
}

impl RootObject {
    pub fn new(input_task: u32, sound_task: u32) -> Self {
        Self {
            base: BaseObject::new(),
            input_task,
            sound_task,
        }
    }
}

impl TaskObject for RootObject {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.base.update_body(host, increment, budget)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<CommandOutcome> {
        let propagate = self.base.control.dispatch(
            kind,
            &mut self.base.clock,
            command,
            output,
            |_, _, command, output| {
                match command.code {
                    1..=3 => return Ok(true),
                    34 => *output = self.input_task,
                    35 => *output = self.sound_task,
                    32 | 33 | 37..=40 => {
                        return Err(Error::new(0, "根对象专属命令尚未接入"));
                    }
                    _ => {}
                }
                Ok(false)
            },
        )?;
        Ok(CommandOutcome::finished(propagate))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
}
