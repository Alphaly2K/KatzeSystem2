use super::{LoopPoints, SoundChannel, channel};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 循环范围、六十四个物理通道及其原始累计时刻。
pub struct SoundState {
    loops: LoopPoints,
    channels: [SoundChannel; 64],
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            loops: LoopPoints::default(),
            channels: std::array::from_fn(|_| SoundChannel::default()),
        }
    }
}

impl SoundState {
    pub fn loop_points(&self) -> &LoopPoints {
        &self.loops
    }

    pub fn channel_time(&self, channel: u32) -> Result<u32> {
        Ok(self.channel(channel)?.clock.elapsed)
    }

    pub fn channel(&self, channel: u32) -> Result<&SoundChannel> {
        self.channels
            .get(channel as usize)
            .ok_or_else(|| Error::new(0, "声音通道编号超出范围"))
    }

    fn update_idle(&mut self, now: u32) {
        for channel in &mut self.channels {
            let clock = &mut channel.clock;
            let previous = if clock.previous == u32::MAX {
                now.wrapping_sub(1)
            } else {
                clock.previous
            };
            clock.previous = now;
            clock.elapsed = clock.elapsed.wrapping_add(now.wrapping_sub(previous));
        }
    }

    fn unload(&mut self, channel: u32) {
        if let Some(sound) = self.channels.get_mut(channel as usize) {
            let linked = sound.linked;
            sound.unload();
            if let Some(sound) = linked.and_then(|index| self.channels.get_mut(index as usize)) {
                sound.unload();
            }
        }
    }

    fn duration_ms(&self, channel: u32) -> u32 {
        self.channels.get(channel as usize).map_or(0, |sound| {
            self.channels[sound.linked.unwrap_or(channel) as usize].duration_ms()
        })
    }

    fn property_channel(&self, channel: u32) -> Option<usize> {
        self.channels
            .get(channel as usize)
            .map(|sound| sound.linked.unwrap_or(channel) as usize)
    }

    fn set_volume(&mut self, channel: u32, volume: i32) {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            sound.requested_volume = volume as f32;
            if sound.pcm.is_some() {
                sound.pcm_volume = volume;
            }
        }
    }

    fn set_pan(&mut self, channel: u32, pan: i32) {
        if let Some(index) = self.property_channel(channel) {
            self.channels[index].pcm_pan = pan.clamp(-100, 100);
        }
    }

    fn load(&mut self, record: &[u8], host: &mut dyn Host, output: &mut u32) -> Result<()> {
        let record = read::range(record, 0, 0x120)?;
        let logical = u32::from_le_bytes(record[..4].try_into().unwrap());
        let options = u32::from_le_bytes(record[0x114..0x118].try_into().unwrap());
        let mode = i32::from_le_bytes(record[0x118..0x11c].try_into().unwrap());
        let duration = i32::from_le_bytes(record[0x11c..0x120].try_into().unwrap());
        let physical = logical
            .checked_add(if mode == 0 { 0 } else { 32 })
            .filter(|&index| index < 64)
            .ok_or_else(|| Error::new(0, "声音装载通道或备用通道超出范围"))?;
        let name = &record[12..272];
        let length = name
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| Error::new(12, "声音装载名称缺少零结尾"))?;
        let name = &name[..length];
        let services = host
            .resource_services()
            .ok_or_else(|| Error::new(0, "宿主未提供声音资源服务"))?;
        let sound = &mut self.channels[physical as usize];
        sound.file.clear();
        sound.pcm = None;
        sound.pcm = channel::load_pcm(services, name)?;
        if sound.pcm.is_some() {
            sound.file.extend_from_slice(name);
            sound.pcm_volume = 100;
            sound.pcm_pan = 0;
        }
        *output = u32::from(sound.pcm.is_some());
        sound.clock.elapsed = 0;
        sound.loop_range = [0, -1];
        if u32::from_le_bytes(record[0x110..0x114].try_into().unwrap()) != 0 {
            if let Some(point) = self.loops.find(name) {
                sound.loop_range = [point.start, point.end];
            }
        }
        sound.parameters = [mode, duration];
        sound.load_options = options;
        sound.linked = None;
        if mode != 0 {
            self.channels[logical as usize].linked = Some(physical);
        }
        channel::check_metadata(services, name)
    }
}

/// 类型 13 的声音任务，持有通道 PCM、装载参数及循环范围。
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
        match command.code {
            180 => {
                if command.data[0] < 64 {
                    let bytes = buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "声音装载缺少记录缓冲区"))?;
                    self.audio.load(bytes, host, output)?;
                }
                return Ok(CommandOutcome::finished(false));
            }
            182 => {
                self.audio.unload(command.data[0]);
                return Ok(CommandOutcome::finished(false));
            }
            187 => {
                self.audio
                    .set_volume(command.data[0], command.data[1] as i32);
                return Ok(CommandOutcome::finished(false));
            }
            190 => {
                if host.sound_stereo() != 0 {
                    self.audio.set_pan(command.data[0], command.data[1] as i32);
                }
                return Ok(CommandOutcome::finished(false));
            }
            188 | 191 => {
                *output = self
                    .audio
                    .property_channel(command.data[0])
                    .and_then(|index| {
                        let sound = &self.audio.channels[index];
                        if command.code == 188 {
                            sound.volume()
                        } else {
                            sound.pan()
                        }
                    })
                    .unwrap_or(0) as u32;
                return Ok(CommandOutcome::finished(false));
            }
            201 => {
                *output = self.audio.duration_ms(command.data[0]);
                return Ok(CommandOutcome::finished(false));
            }
            _ => {}
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
