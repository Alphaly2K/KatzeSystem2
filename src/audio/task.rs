use super::{LoopPoints, Pcm, SoundChannel, SoundGroups, channel, playback::Playback};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 循环范围、六十四个物理通道及其原始累计时刻。
pub struct SoundState {
    loops: LoopPoints,
    channels: [SoundChannel; 64],
    groups: Option<BTreeMap<u32, GroupRange>>,
}

struct GroupRange {
    id: u32,
    range: [i32; 2],
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            loops: LoopPoints::default(),
            channels: std::array::from_fn(|_| SoundChannel::default()),
            groups: None,
        }
    }
}

impl SoundState {
    /// 捕获组范围；同一表键保留首次配置，之后的全局标签变化不修改此快照。
    pub fn set_groups(&mut self, groups: &SoundGroups) -> Result<()> {
        if groups.entries.len() > read::MAX_ITEMS {
            return Err(Error::new(0, "声音组数量超过上限"));
        }
        let mut ranges = BTreeMap::new();
        for group in &groups.entries {
            ranges.entry(group.key).or_insert(GroupRange {
                id: group.id,
                range: group.range,
            });
        }
        self.groups = Some(ranges);
        Ok(())
    }

    pub fn group_range(&self, key: u32) -> Option<[i32; 2]> {
        self.groups.as_ref()?.get(&key).map(|group| group.range)
    }

    fn select_channel(&mut self, range: [i32; 2]) -> Result<u32> {
        let [first, last] = range.map(|value| value.clamp(0, 64) as u32);
        for index in first..=last {
            if !self.channel(index)?.is_playing() {
                return Ok(index);
            }
        }
        let mut selected = first;
        'priority: for flag in 0..2 {
            for index in first..=last {
                if self.channel(index)?.allocation_flags[flag] != 0 {
                    selected = index;
                    break 'priority;
                }
            }
        }
        let sound = self
            .channels
            .get_mut(selected as usize)
            .ok_or_else(|| Error::new(0, "声音组选择超出物理通道容量"))?;
        sound.file.clear();
        sound.pcm = None;
        sound.playback = Playback::default();
        sound.clock.elapsed = 0;
        sound.loop_range[0] = 0;
        sound.allocation_flags = [0; 2];
        Ok(selected)
    }

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

    /// 读取物理通道的源格式采样并推进位置；空闲及结束后的部分补零。
    pub fn read_channel(&mut self, channel: u32, frames: usize) -> Result<Pcm> {
        let sound = self
            .channels
            .get_mut(channel as usize)
            .ok_or_else(|| Error::new(0, "声音通道编号超出范围"))?;
        let pcm = sound
            .pcm
            .as_ref()
            .ok_or_else(|| Error::new(0, "声音通道尚未装载 PCM"))?;
        let count = frames
            .checked_mul(pcm.channels() as usize)
            .filter(|count| *count <= read::MAX_BYTES / 2)
            .ok_or_else(|| Error::new(0, "声音采样输出超过容量上限"))?;
        let mut output = vec![0; count];
        sound.playback.read(pcm, &mut output)?;
        Pcm::new(pcm.sample_rate(), pcm.channels(), output)
    }

    fn update_idle(&mut self, now: u32, stereo: bool) {
        for channel in &mut self.channels {
            channel.update_controls(now, stereo);
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

    fn unload(&mut self, channel: u32) -> Result<()> {
        let Some(sound) = self.channels.get(channel as usize) else {
            return Ok(());
        };
        let linked = sound.linked;
        for index in [Some(channel), linked].into_iter().flatten() {
            let sound = &mut self.channels[index as usize];
            if sound.grouped {
                if sound.is_playing() && sound.allocation_flags[0] == 0 {
                    return Err(Error::new(0, "声音分组延迟释放尚未接入"));
                }
                sound.started_tick = u32::MAX;
                sound.clock.previous = 0;
            } else {
                sound.unload();
            }
        }
        Ok(())
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
            sound.ramps.active &= !1;
            sound.requested_volume = volume as f32;
            if sound.pcm.is_some() {
                sound.pcm_volume = volume;
            }
        }
    }

    fn set_pan(&mut self, channel: u32, pan: i32, stereo: bool) {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            sound.ramps.active &= !2;
            if stereo {
                sound.pcm_pan = pan.clamp(-100, 100);
            }
        }
    }

    fn start_ramp(&mut self, channel: u32, packed: u32, pan: bool, now: u32) -> Result<()> {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if !pan && sound.parameters[0] != 0 {
                return Err(Error::new(0, "声音备用通道淡变完成与延迟释放尚未接入"));
            }
            let slot = usize::from(pan);
            sound.ramps.active |= 1 << slot;
            sound.ramps.target[slot] = if pan {
                packed as i16 as f32
            } else {
                packed as u16 as f32
            };
            sound.ramps.current[slot] = if pan {
                sound.pan().unwrap_or(0) as f32
            } else {
                sound.volume().unwrap_or(0) as u32 as f32
            };
            sound.ramps.remaining[slot] = (packed >> 16) as f32;
            sound.ramps.previous = now;
        }
        Ok(())
    }

    fn start(&mut self, channel: u32, count: u32, tick: u32) -> Result<()> {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.parameters[0] != 0 {
                return Err(Error::new(0, "声音淡变播放尚未接入"));
            }
            sound.ramps.active = 0;
            if let Some(pcm) = &sound.pcm {
                sound
                    .playback
                    .start(pcm, count, playback_range(sound, count))?;
                sound.pcm_volume = super::control::truncate(sound.requested_volume);
            }
            if sound.started_tick == u32::MAX {
                sound.started_tick = tick;
            }
        }
        Ok(())
    }

    fn control_pair(&mut self, channel: u32, command: u32, tick: u32) -> Result<()> {
        let Some(sound) = self.channels.get(channel as usize) else {
            return Ok(());
        };
        let linked = sound.linked;
        for index in [Some(channel), linked].into_iter().flatten() {
            let sound = &mut self.channels[index as usize];
            if matches!(command, 184 | 185) {
                sound.ramps.active = 0;
            }
            match command {
                184 => sound.playback.pause(),
                185 => {
                    if let Some(pcm) = &sound.pcm {
                        sound.playback.stop(pcm)?;
                    }
                }
                218 if sound.started_tick == u32::MAX => sound.started_tick = tick,
                _ => (),
            }
        }
        Ok(())
    }

    fn is_playing(&self, channel: u32) -> bool {
        self.channels.get(channel as usize).is_some_and(|sound| {
            sound.is_playing()
                || sound
                    .linked
                    .is_some_and(|index| self.channels[index as usize].is_playing())
        })
    }

    fn change_count(&mut self, channel: u32, count: u32) -> Result<()> {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.pcm.is_some() {
                sound
                    .playback
                    .change_count(count, playback_range(sound, count));
            }
        }
        Ok(())
    }

    fn load(
        &mut self,
        record: &[u8],
        host: &mut dyn Host,
        output: &mut u32,
        code: u32,
    ) -> Result<()> {
        let grouped = code >= 206;
        let whole = code & 1 != 0;
        let logical = if grouped {
            let groups = self
                .groups
                .as_ref()
                .ok_or_else(|| Error::new(0, "声音任务尚未配置组范围"))?;
            let id = read::u32_at(record, if code <= 207 { 4 } else { 8 })?;
            *output = u32::MAX;
            let group = if code <= 207 {
                groups.get(&id)
            } else {
                groups.values().find(|group| group.id == id)
            };
            let Some(group) = group else {
                return Ok(());
            };
            self.select_channel(group.range)?
        } else {
            let logical = read::u32_at(record, 0)?;
            if code == 181 && logical as i32 >= 64 {
                return Ok(());
            }
            logical
        };
        let record = read::range(record, 0, 0x120)?;
        let options = u32::from_le_bytes(record[0x114..0x118].try_into().unwrap());
        let mode = i32::from_le_bytes(record[0x118..0x11c].try_into().unwrap());
        let duration = i32::from_le_bytes(record[0x11c..0x120].try_into().unwrap());
        let offset = if mode == 0 { 0 } else { 32 };
        let physical = if code == 181 {
            Some(logical.wrapping_add(offset))
        } else {
            logical.checked_add(offset)
        }
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
        sound.playback = Playback::default();
        if let Some(pcm) = channel::load_pcm(services, name)? {
            let playback = Playback::new(&pcm, whole, options)?;
            sound.file.extend_from_slice(name);
            sound.pcm_volume = 100;
            sound.pcm_pan = 0;
            sound.distance_range = [100.0; 2];
            sound.position = [0.0; 3];
            sound.pcm = Some(pcm);
            sound.playback = playback;
        }
        if grouped && sound.pcm.is_none() {
            return Ok(());
        }
        *output = if grouped {
            physical
        } else {
            u32::from(sound.pcm.is_some())
        };
        sound.clock.elapsed = 0;
        sound.ramps.active = 0;
        // 209 保留分配后的循环缓存，包括全忙回收只清起点的结果。
        if code != 209 {
            sound.loop_range = [0, -1];
            if !whole && u32::from_le_bytes(record[0x110..0x114].try_into().unwrap()) != 0 {
                if let Some(point) = self.loops.find(name) {
                    sound.loop_range = [point.start, point.end];
                }
            }
        }
        sound.parameters = [mode, duration];
        sound.load_options = options;
        sound.started_tick = u32::MAX;
        sound.linked = None;
        sound.grouped = grouped;
        sound.allocation_flags = [0; 2];
        if mode != 0 && (grouped || whole) {
            sound.linked = Some(physical);
        } else if mode != 0 {
            self.channels[logical as usize].linked = Some(physical);
        }
        channel::check_metadata(services, name)
    }
}

fn playback_range(sound: &SoundChannel, count: u32) -> [i64; 2] {
    if count == 1 {
        [0, -1]
    } else {
        sound.loop_range
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
    fn audio_state_mut(&mut self) -> Option<&mut SoundState> {
        Some(&mut self.audio)
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.base.control.deletion_requested == 0 && self.enabled {
            self.audio
                .update_idle(self.base.clock.previous_time, host.sound_stereo() != 0);
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
            180 | 181 => {
                if command.code == 181 || command.data[0] < 64 {
                    let bytes = buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "声音装载缺少记录缓冲区"))?;
                    self.audio.load(bytes, host, output, command.code)?;
                }
                return Ok(CommandOutcome::finished(false));
            }
            206..=209 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "声音分组装载缺少记录缓冲区"))?;
                self.audio.load(bytes, host, output, command.code)?;
                return Ok(CommandOutcome::finished(false));
            }
            210 => {
                let name = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "声音组查询缺少名称缓冲区"))?;
                let end = name
                    .iter()
                    .position(|&byte| byte == 0)
                    .ok_or_else(|| Error::new(0, "声音组名称缺少零结尾"))?;
                *output = host.sound_group_id(&name[..end])?;
                return Ok(CommandOutcome::finished(false));
            }
            182 => {
                self.audio.unload(command.data[0])?;
                return Ok(CommandOutcome::finished(false));
            }
            183 => {
                self.audio
                    .start(command.data[0], command.data[1], host.system_time_ms())?;
                return Ok(CommandOutcome::finished(false));
            }
            184 | 185 | 218 => {
                self.audio
                    .control_pair(command.data[0], command.code, host.system_time_ms())?;
                return Ok(CommandOutcome::finished(false));
            }
            186 => {
                *output = u32::from(self.audio.is_playing(command.data[0]));
                return Ok(CommandOutcome::finished(false));
            }
            219 => {
                self.audio.change_count(command.data[0], command.data[1])?;
                return Ok(CommandOutcome::finished(false));
            }
            187 => {
                self.audio
                    .set_volume(command.data[0], command.data[1] as i32);
                return Ok(CommandOutcome::finished(false));
            }
            190 => {
                self.audio.set_pan(
                    command.data[0],
                    command.data[1] as i32,
                    host.sound_stereo() != 0,
                );
                return Ok(CommandOutcome::finished(false));
            }
            189 | 192 => {
                self.audio.start_ramp(
                    command.data[0],
                    command.data[1],
                    command.code == 192,
                    self.base.clock.elapsed(),
                )?;
                return Ok(CommandOutcome::finished(false));
            }
            193 | 194 => {
                if let Some(index) = self.audio.property_channel(command.data[0]) {
                    if let Some(bytes) = buffers.data1.as_deref() {
                        let bytes =
                            read::range(bytes, 0, if command.code == 193 { 8 } else { 12 })?;
                        let values: Vec<_> = bytes
                            .chunks_exact(4)
                            .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                            .collect();
                        let sound = &mut self.audio.channels[index];
                        if sound.pcm.is_some() {
                            if command.code == 193 {
                                sound.distance_range[1] = values[1];
                            } else if host.sound_stereo() != 0 {
                                sound.set_position([values[0], values[1], values[2]]);
                            }
                        }
                    } else if command.data[1] != 0 {
                        return Err(Error::new(0, "声音空间设置缺少记录缓冲区"));
                    }
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
    fn external_storage(&self) -> crate::memory::SharedMemory {
        self.fallback.external_storage()
    }

    fn create_unloaded_ordinary(&mut self, id: u32) -> Result<Box<dyn TaskObject>> {
        self.fallback.create_unloaded_ordinary(id)
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<crate::kcs::Program>> {
        self.fallback.load_kcs_program(name)
    }

    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::SOUND {
            Ok(Box::new(SoundObject::new(self.enabled)))
        } else {
            self.fallback.create(id, object_kind, name)
        }
    }
}
