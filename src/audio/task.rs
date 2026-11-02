use super::LoopPoints;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

#[derive(Clone, Copy, Default)]
struct ChannelClock {
    previous: u32,
    elapsed: u32,
}

/// 循环范围和六十四个空闲通道的原始累计时刻。
pub struct SoundState {
    loops: LoopPoints,
    channels: [ChannelClock; 64],
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            loops: LoopPoints::default(),
            channels: [ChannelClock::default(); 64],
        }
    }
}

impl SoundState {
    pub fn loop_points(&self) -> &LoopPoints {
        &self.loops
    }

    pub fn channel_time(&self, channel: u32) -> Result<u32> {
        self.channels
            .get(channel as usize)
            .map(|clock| clock.elapsed)
            .ok_or_else(|| Error::new(0, "声音通道编号超出范围"))
    }

    fn update_idle(&mut self, now: u32) {
        for clock in &mut self.channels {
            let previous = if clock.previous == u32::MAX {
                now.wrapping_sub(1)
            } else {
                clock.previous
            };
            clock.previous = now;
            clock.elapsed = clock.elapsed.wrapping_add(now.wrapping_sub(previous));
        }
    }
}

/// 类型 13 的声音任务，接收循环范围登记并维护空闲通道时刻。
pub struct SoundObject {
    base: BaseObject,
    audio: SoundState,
    enabled: bool,
}

impl SoundObject {
    /// enabled 对应构造时声音管理器是否存在，与任务时钟暂停分别保存。
    pub fn new(enabled: bool) -> Self {
        Self {
            base: BaseObject::new(),
            audio: SoundState::default(),
            enabled,
        }
    }

    pub fn audio(&self) -> &SoundState {
        &self.audio
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

impl TaskObject for SoundObject {
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
    fn audio_state(&self) -> Option<&SoundState> {
        Some(&self.audio)
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.base.control.deletion_requested == 0 && self.enabled {
            self.audio.update_idle(self.base.clock.previous_time);
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
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        if !self.enabled {
            *output = 0;
            return Ok(CommandOutcome::finished(false));
        }
        if command.code == 199 {
            if let Some(bytes) = buffers.data1.as_deref() {
                let bytes = read::range(bytes, 0, 48)?;
                let start = i64::from_le_bytes(bytes[32..40].try_into().unwrap());
                let end = i64::from_le_bytes(bytes[40..48].try_into().unwrap());
                self.audio.loops.register(&bytes[..32], start, end)?;
            }
            return Ok(CommandOutcome::finished(false));
        }
        if matches!(command.code, 19 | 180..=229) {
            return Err(Error::new(
                0,
                format!("声音对象命令 {} 尚未接入", command.code),
            ));
        }
        self.base
            .dispatch_command(kind, command, output, host, budget)
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
        if self.enabled && self.base.control.deletion_requested == 0 {
            self.base.request_delete(host, budget)?;
        }
        Ok(true)
    }
}

pub struct SoundFactory<F> {
    pub enabled: bool,
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for SoundFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::SOUND {
            Ok(Box::new(SoundObject::new(self.enabled)))
        } else {
            self.fallback.create(id, object_kind, name)
        }
    }
}
