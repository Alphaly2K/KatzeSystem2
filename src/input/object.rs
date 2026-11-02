use super::InputState;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 输入对象的命令、显式事件入口和同步设备更新。
pub struct InputObject {
    id: u32,
    state: InputState,
}

impl InputObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            state: InputState::default(),
        }
    }
}

impl Default for InputObject {
    fn default() -> Self {
        Self::new(0)
    }
}

impl TaskObject for InputObject {
    fn control(&self) -> &Control {
        &self.state.commands.control
    }
    fn clock(&self) -> &TaskClock {
        &self.state.commands.clock
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        (
            &mut self.state.commands.control,
            &mut self.state.commands.clock,
        )
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn input_state(&self) -> Option<&InputState> {
        Some(&self.state)
    }
    fn input_state_mut(&mut self) -> Option<&mut InputState> {
        Some(&mut self.state)
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        if context.is_empty() {
            Ok(())
        } else {
            Err(Error::new(0, "输入对象创建上下文尚未接入"))
        }
    }

    fn update_body(
        &mut self,
        host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        self.state.update_device(self.id, host)?;
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }

    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        _budget: usize,
    ) -> Result<CommandOutcome> {
        self.state
            .dispatch_command(kind, command, buffers, output, host)
            .map(CommandOutcome::finished)
    }

    fn enqueue_message(&mut self, _message: [u32; 4]) -> Result<()> {
        Err(Error::new(0, "输入对象不接收普通 KCS 队列消息"))
    }

    fn dispatch_message(
        &mut self,
        _message: &mut [u32; 4],
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<bool> {
        Err(Error::new(0, "输入对象不接收普通 KCS 同步消息"))
    }

    fn request_delete(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        self.state.commands.request_delete(host.input_device())?;
        Ok(true)
    }
}

/// 创建输入、矩形按钮及滑动控件任务，其余类型委托已有工厂。
pub struct InputFactory<F> {
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for InputFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        match object_kind {
            kind::INPUT => Ok(Box::new(InputObject::new(id))),
            kind::BUTTON => Ok(Box::new(super::ButtonObject::new(id))),
            kind::SLIDER => Ok(Box::new(super::SliderObject::new(id))),
            kind::SCROLL2 => Ok(Box::new(super::Scroll2Object::new(id))),
            _ => self.fallback.create(id, object_kind, name),
        }
    }
}
