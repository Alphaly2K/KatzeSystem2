use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 类型 38 的粒子控制状态；粒子装载、播放及绘制尚未接入。
pub struct ParticleObject {
    base: BaseObject,
    context: Vec<u8>,
    opacity: u32,
    transform_source: u32,
    playing: u32,
    parameters: Option<[u32; 32]>,
}

impl ParticleObject {
    pub fn new(_id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            context: Vec::new(),
            opacity: 256,
            transform_source: u32::MAX,
            playing: 0,
            parameters: None,
        }
    }

    /// 不透明度保留有符号输入，255 及更大正值转换为 256。
    pub fn opacity(&self) -> u32 {
        self.opacity
    }

    /// 变换来源编号；u32::MAX 表示关闭外部变换来源。
    pub fn transform_source(&self) -> u32 {
        self.transform_source
    }

    /// 命令 325 保存的 32 项原始参数。
    pub fn parameters(&self) -> Option<&[u32; 32]> {
        self.parameters.as_ref()
    }
}

impl TaskObject for ParticleObject {
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
        self.context = context.to_vec();
        Ok(())
    }

    fn update_body(&mut self, _host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
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
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        match command.code {
            324 => *output = self.playing,
            325 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .and_then(|bytes| bytes.get(..128))
                    .ok_or_else(|| Error::new(0, "粒子参数命令需要 128 字节缓冲区"))?;
                self.parameters = Some(std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                }));
            }
            326 => {
                let value = command.data[1];
                self.opacity = if (value as i32) < 255 { value } else { 256 };
            }
            327 => {
                let value = command.data[1];
                self.transform_source = if value == 0 { u32::MAX } else { value };
            }
            320..=323 | 591 | 592 => {
                return Err(Error::new(0, "粒子装载、播放及绘制命令尚未接入"));
            }
            _ => {
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
            }
        }
        Ok(CommandOutcome::finished(false))
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
