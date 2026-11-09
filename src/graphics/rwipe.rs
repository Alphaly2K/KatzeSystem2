use crate::Result;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};

/// 类型 39 的遮罩划扫转场任务。
///
/// 原生 `kcTaskRenderWipe` 的命令协议按指令核对：390 写入一份 16 字节参数块，391 写入一份
/// 原始字，393 写入一份 20 字节配置（速度、时长、遮罩资源、扩展字与变体标记），394 设置
/// 运行状态并在启动时登记期限，383 把运行状态写回输出。期限到达后转场完成，状态归零。
/// 转场的实际绘制尚未接入帧缓冲。
pub struct RWipeObject {
    base: BaseObject,
    params: Option<Vec<u8>>,
    aux_block: Option<Vec<u8>>,
    aux_word: u32,
    state: u32,
    deadline: Option<u32>,
}

impl Default for RWipeObject {
    fn default() -> Self {
        Self::new()
    }
}

impl RWipeObject {
    pub fn new() -> Self {
        Self {
            base: BaseObject::new(),
            params: None,
            aux_block: None,
            aux_word: 0,
            state: 0,
            deadline: None,
        }
    }
}

impl TaskObject for RWipeObject {
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
    fn update_body(&mut self, _host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.state != 0
            && self
                .deadline
                .is_some_and(|deadline| self.base.clock.elapsed() >= deadline)
        {
            self.state = 0;
            self.deadline = None;
        }
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
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.base.control().deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let handled =
            match command.code {
                383 => {
                    *output = self.state;
                    true
                }
                390 => {
                    if let Some(block) = buffers.data1.as_deref() {
                        self.aux_block = Some(block.to_vec());
                    }
                    true
                }
                391 => {
                    self.aux_word = command.data[1];
                    true
                }
                393 => {
                    if let Some(block) = buffers.data1.as_deref() {
                        self.params = Some(block.to_vec());
                        if self.state != 0 {
                            self.deadline =
                                Some(self.base.clock.elapsed().wrapping_add(
                                    crate::read::u32_at(block, 8).unwrap_or_default(),
                                ));
                        }
                    }
                    true
                }
                394 => {
                    self.state = command.data[1];
                    if self.state != 0 {
                        let duration = self
                            .params
                            .as_deref()
                            .and_then(|block| crate::read::u32_at(block, 8).ok())
                            .unwrap_or_default();
                        self.deadline = Some(self.base.clock.elapsed().wrapping_add(duration));
                    }
                    true
                }
                _ => false,
            };
        if !handled {
            return self
                .base
                .dispatch_command(kind, command, output, host, budget);
        }
        Ok(CommandOutcome::finished(true))
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
