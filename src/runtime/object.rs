use crate::kcs::{Host, Interpreter, OrdinaryTask};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 任务树中的对象协议。图形、音频、输入等类型由对应子系统提供实现。
///
/// 除对象命令外，返回 `false` 的方法表示本次调用预算耗尽，调用者应继续传入相同输入完成续跑。
pub trait TaskObject {
    fn control(&self) -> &Control;
    fn clock(&self) -> &TaskClock;

    /// 生命周期调度读取的基类删除标记与整数时钟。
    fn state(&mut self) -> (&mut Control, &mut TaskClock);

    /// 脚本 VM；没有脚本执行能力的对象返回 `None`。
    fn interpreter(&self) -> Option<&Interpreter>;
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter>;

    /// 支持命令 25 的原始创建上下文；没有上下文存储的对象返回 None。
    fn creation_context(&self) -> Option<&[u8]> {
        None
    }

    /// 显式输入入口使用的状态；其他对象返回 None。
    fn input_state(&self) -> Option<&crate::input::InputState> {
        None
    }
    fn input_state_mut(&mut self) -> Option<&mut crate::input::InputState> {
        None
    }

    fn audio_state(&self) -> Option<&crate::audio::SoundState> {
        None
    }

    fn audio_state_mut(&mut self) -> Option<&mut crate::audio::SoundState> {
        None
    }

    /// 支持执行中任务操作的对象提供队列视图，并在推进 VM 后消费宿主的 TaskEffects。
    fn queued_messages(&self) -> Option<&[[u32; 4]]> {
        None
    }

    /// 消费 TaskEffects 删除标记的对象可在嵌套调用期间接收删除请求。
    fn accepts_deferred_delete(&self) -> bool {
        false
    }

    /// 时钟和子任务更新前执行的主体前置处理。
    fn begin_update(&mut self) {}

    /// 接收创建命令 26 传递的原始上下文；对象类型不支持时返回诊断。
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()>;

    /// 执行对象主体；在返回前完成所有预算续跑由调用者负责。
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool>;

    /// 处理对象命令；`kind` 为树中登记的对象类型。完成与传播状态分别返回。
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome>;

    /// 携带显式数据缓冲区的同一命令协议；纯标量处理器继续使用普通入口。
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        _buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command(kind, command, output, host, budget)
    }

    /// 复制入队一条任务消息；不推进 VM。
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()>;

    /// 同步投递任务消息。
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool>;

    /// 删除命令 3 的专属通知；完成时对象已经设置删除标记。
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool>;

    /// 直属子任务已收到删除请求；按对象类型处理自身通知及标记。
    fn mark_destroying(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.request_delete(host, budget)
    }

    /// 子树已经释放；释放对象持有的资源和执行状态。
    fn prepare_release(&mut self, _host: &mut dyn Host, _budget: usize) -> Result<bool> {
        Ok(true)
    }

    /// 宿主登记已解除；释放对象自身的资源和执行状态。
    fn release(&mut self) -> Result<()> {
        Ok(())
    }
}

/// 只有基类控制与时钟的对象。
pub struct BaseObject {
    pub control: Control,
    pub clock: TaskClock,
}

impl BaseObject {
    pub fn new() -> Self {
        Self {
            control: Control::default(),
            clock: TaskClock::default(),
        }
    }
}

impl Default for BaseObject {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskObject for BaseObject {
    fn control(&self) -> &Control {
        &self.control
    }

    fn clock(&self) -> &TaskClock {
        &self.clock
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        (&mut self.control, &mut self.clock)
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    /// 基类没有上下文存储；空正文无需保存，非空正文不能被静默丢弃。
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        if context.is_empty() {
            return Ok(());
        }
        Err(Error::new(0, "基类对象不保存创建上下文"))
    }

    fn update_body(
        &mut self,
        _host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<CommandOutcome> {
        // 基类命令消费当前分支；未识别编号不修改状态，允许继续传播。
        let propagate =
            self.control
                .dispatch(kind, &mut self.clock, command, output, |_, _, _, _| {
                    Ok(true)
                })?;
        Ok(CommandOutcome::finished(propagate))
    }

    fn enqueue_message(&mut self, _message: [u32; 4]) -> Result<()> {
        Err(Error::new(0, "基类对象不接收任务消息"))
    }

    fn dispatch_message(
        &mut self,
        _message: &mut [u32; 4],
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<bool> {
        Err(Error::new(0, "基类对象不接收任务消息"))
    }

    fn request_delete(&mut self, _host: &mut dyn Host, _budget: usize) -> Result<bool> {
        self.control.deletion_requested = 1;
        Ok(true)
    }
}

/// 已装载程序、时钟和消息队列的普通 KCS 对象。
pub struct OrdinaryObject {
    pub task: OrdinaryTask,
}

impl OrdinaryObject {
    pub fn new(task: OrdinaryTask) -> Self {
        Self { task }
    }
}

impl TaskObject for OrdinaryObject {
    fn creation_context(&self) -> Option<&[u8]> {
        Some(&self.task.creation_context)
    }

    fn queued_messages(&self) -> Option<&[[u32; 4]]> {
        Some(self.task.queued_messages())
    }

    fn control(&self) -> &Control {
        &self.task.control
    }

    fn clock(&self) -> &TaskClock {
        &self.task.clock
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        (&mut self.task.control, &mut self.task.clock)
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        Some(&self.task.interpreter)
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        Some(&mut self.task.interpreter)
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.task.set_creation_context(Some(context))
    }

    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        let mut host = host;
        self.task.update_body(increment, &mut host, budget)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        let mut host = host;
        self.task
            .dispatch_control(kind, command, output, &mut host, budget)
    }

    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if command.code == 177 {
            super::save::query_shared_numbers(self.task.control.deletion_requested != 0, buffers)?;
            return Ok(CommandOutcome::finished(false));
        }
        self.dispatch_command(kind, command, output, host, budget)
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.task.enqueue_message(message)
    }

    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        let mut host = host;
        self.task
            .dispatch_message(message, output, &mut host, budget)
    }

    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        let mut host = host;
        self.task.request_delete(&mut host, budget)
    }

    fn release(&mut self) -> Result<()> {
        if self.task.is_pending() {
            return Err(Error::new(self.task.interpreter.pc, "普通任务执行尚未完成"));
        }
        Ok(())
    }
}
