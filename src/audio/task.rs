use super::{
    LoopPoints, Pcm, SoundChannel, SoundGroups, channel, output::Action, playback::Playback,
};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 循环范围、六十四个物理通道及其原始累计时刻。
pub struct SoundState {
    loops: LoopPoints,
    pub(super) channels: [SoundChannel; 64],
    groups: Option<BTreeMap<u32, GroupRange>>,
    pub(super) output: Option<super::output::Link>,
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
            output: None,
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
        sound.resampler = super::mix::Resampler::default();
        sound.clock.elapsed = 0;
        sound.loop_range[0] = 0;
        sound.allocation_flags = [0; 2];
        self.output_replace(selected as usize);
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
        if self.output.is_some() {
            return Err(Error::new(0, "声音采样已交给独立混音器"));
        }
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
        sound.resampler = super::mix::Resampler::default();
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
        self.output_gains();
    }

    fn configuration_changed(&mut self) {
        for logical in 0..self.channels.len() {
            if !self.channels[logical].grouped {
                continue;
            }
            let linked = self.channels[logical].linked;
            for (slot, index) in [Some(logical as u32), linked]
                .into_iter()
                .flatten()
                .enumerate()
            {
                if self.channels[index as usize].is_playing() {
                    // 基线写回位置由关联对中的查询序号决定。
                    self.channels[slot].allocation_flags[0] = 1;
                    self.channels[slot].ramps.remaining[0] = 0.0;
                }
            }
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
                if sound.is_playing() {
                    // 播放中置第一类分配标记即启动延迟释放；已有标记跳过新请求。
                    if sound.allocation_flags[0] == 0 {
                        sound.allocation_flags[0] = 1;
                    }
                    continue;
                }
                sound.started_tick = u32::MAX;
                sound.clock.previous = 0;
            } else {
                sound.unload();
                self.output_replace(index as usize);
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
        self.output_gains();
    }

    fn set_pan(&mut self, channel: u32, pan: i32, stereo: bool) {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            sound.ramps.active &= !2;
            if stereo {
                sound.pcm_pan = pan.clamp(-100, 100);
            }
        }
        self.output_gains();
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
        self.sync_output();
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.parameters[0] != 0 {
                return Err(Error::new(0, "声音淡变播放尚未接入"));
            }
            sound.ramps.active = 0;
            if let Some(pcm) = &sound.pcm {
                if sound.playback.playing || !sound.resampler.paused {
                    sound.resampler = super::mix::Resampler::default();
                }
                sound.resampler.paused = false;
                sound
                    .playback
                    .start(pcm, count, playback_range(sound, count))?;
                sound.pcm_volume = super::control::truncate(sound.requested_volume);
            }
            if sound.started_tick == u32::MAX {
                sound.started_tick = tick;
            }
            let range = playback_range(sound, count);
            self.output_gains();
            self.output_action(index, Action::Start(count, range));
        }
        Ok(())
    }

    fn control_pair(&mut self, channel: u32, command: u32, tick: u32) -> Result<()> {
        self.sync_output();
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
                184 => {
                    sound.playback.pause();
                    sound.resampler.paused = true;
                }
                185 => {
                    sound.resampler = super::mix::Resampler::default();
                    if let Some(pcm) = &sound.pcm {
                        sound.playback.stop(pcm)?;
                    }
                }
                218 if sound.started_tick == u32::MAX => sound.started_tick = tick,
                _ => (),
            }
            match command {
                184 => self.output_action(index as usize, Action::Pause),
                185 => self.output_action(index as usize, Action::Stop),
                _ => (),
            }
        }
        Ok(())
    }

    /// 查询逻辑通道关联的 TAG；全一选择首个含该名称的物理通道。
    pub fn tag_value(&self, channel: u32, name: &[u8]) -> u32 {
        if channel == u32::MAX {
            self.channels
                .iter()
                .find_map(|sound| sound.tags.get(name).copied())
                .unwrap_or(0)
        } else {
            self.property_channel(channel)
                .and_then(|index| self.channels[index].tags.get(name).copied())
                .unwrap_or(0)
        }
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
        self.sync_output();
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.pcm.is_some() {
                let range = playback_range(sound, count);
                sound.playback.change_count(count, range);
                self.output_action(index, Action::Count(count, range));
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
        sound.resampler = super::mix::Resampler::default();
        self.output_replace(physical as usize);
        let sound = &mut self.channels[physical as usize];
        if let Some(pcm) = channel::load_pcm(services, name)? {
            let playback = Playback::new(&pcm, whole, options)?;
            sound.file.extend_from_slice(name);
            sound.pcm_volume = 100;
            sound.pcm_pan = 0;
            sound.distance_range = [100.0; 2];
            sound.position = [0.0; 3];
            sound.velocity = [0.0; 3];
            sound.doppler_factor = 1.0;
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
        self.output_replace(physical as usize);
        channel::load_metadata(services, name, &mut self.channels[physical as usize].tags)
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
            19 => {
                if host.jtalk_enabled() == 0 {
                    self.audio.configuration_changed();
                }
                *output = 1;
                return Ok(CommandOutcome::finished(false));
            }
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
            215 => {
                *output = 0;
                let channel = command.data[0];
                if channel == u32::MAX || channel < 64 {
                    let bytes = buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "声音 TAG 查询缺少名称缓冲区"))?;
                    *output = self.audio.tag_value(channel, read::zstr(bytes, 0)?);
                }
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
            193 | 194 | 196 | 197 => {
                if let Some(index) = self.audio.property_channel(command.data[0]) {
                    if let Some(bytes) = buffers.data1.as_deref() {
                        let bytes = read::range(
                            bytes,
                            0,
                            match command.code {
                                193 => 8,
                                197 => 4,
                                _ => 12,
                            },
                        )?;
                        let values: Vec<_> = bytes
                            .chunks_exact(4)
                            .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                            .collect();
                        let sound = &mut self.audio.channels[index];
                        if sound.pcm.is_some() {
                            match command.code {
                                193 => sound.distance_range[1] = values[1],
                                194 if host.sound_stereo() != 0 => {
                                    sound.set_position([values[0], values[1], values[2]]);
                                }
                                196 => sound.velocity = [values[0], values[1], values[2]],
                                197 => sound.doppler_factor = values[0],
                                _ => {}
                            }
                        }
                    } else if command.data[1] != 0 {
                        return Err(Error::new(0, "声音空间设置缺少记录缓冲区"));
                    }
                }
                self.audio.output_gains();
                return Ok(CommandOutcome::finished(false));
            }
            198 => {
                let bytes = buffers
                    .data1
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "声音渐变查询缺少结果缓冲区"))?;
                read::range(bytes, 0, 4)?;
                let value = self
                    .audio
                    .property_channel(command.data[0])
                    .map_or(0, |index| {
                        u32::from(self.audio.channels[index].ramps.active & 1)
                    });
                bytes[..4].copy_from_slice(&value.to_le_bytes());
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
        if matches!(command.code, 180..=229) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{expression::State, kcs::CoreHost};

    fn output_state() -> SoundState {
        let mut sound = SoundState::default();
        let channel = &mut sound.channels[0];
        let pcm = Pcm::new(8, 1, vec![8192, 16384, 24576, 16384]).unwrap();
        channel.playback = Playback::new(&pcm, true, 0).unwrap();
        channel.pcm = Some(pcm);
        channel.requested_volume = 100.0;
        sound
    }

    #[test]
    fn output_mixer_advances_without_task_updates_and_keeps_controls_and_progress() {
        let mut sound = output_state();
        let mut mixer = sound.connect_output().unwrap();
        assert!(sound.connect_output().is_err());
        assert!(sound.read_channel(0, 1).is_err());
        assert!(sound.mix_stereo(8, &mut [0.0; 2]).is_err());
        sound.start(0, 1, 7).unwrap();
        assert!(sound.channel(0).unwrap().is_playing());
        let mut sample = [0.0; 2];
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.25; 2]);
        assert_eq!(sound.channel(0).unwrap().position_frames(), 1);
        sound.control_pair(0, 184, 0).unwrap();
        assert!(!sound.channel(0).unwrap().is_playing());
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.0; 2]);
        sound.start(0, 1, 8).unwrap();
        sound.set_volume(0, 50);
        sound.set_pan(0, -100, true);
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.25, 0.0]);
        let (mut mixer, tail) = std::thread::spawn(move || {
            let mut tail = [0.0; 6];
            mixer.render(8, &mut tail).unwrap();
            (mixer, tail)
        })
        .join()
        .unwrap();
        assert_eq!(tail, [0.375, 0.0, 0.25, 0.0, 0.0, 0.0]);
        assert!(!sound.channel(0).unwrap().is_playing());
        assert_eq!(sound.channel(0).unwrap().completed_passes(), 1);
        sound.set_volume(0, 100);
        sound.set_pan(0, 0, true);
        sound.start(0, 0, 9).unwrap();
        mixer.render(8, &mut [0.0; 18]).unwrap();
        assert!(sound.channel(0).unwrap().is_playing());
        sound.change_count(0, 1).unwrap();
        mixer.render(8, &mut [0.0; 8]).unwrap();
        assert!(!sound.channel(0).unwrap().is_playing());
        sound.start(0, 2, 10).unwrap();
        sound.control_pair(0, 185, 0).unwrap();
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.0; 2]);
        assert_eq!(sound.channel(0).unwrap().position_frames(), 0);
        sound.start(0, 1, 11).unwrap();
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.25; 2]);
        sound.unload(0).unwrap();
        mixer.render(8, &mut sample).unwrap();
        assert_eq!(sample, [0.0; 2]);
    }

    #[test]
    fn pending_output_controls_keep_newer_status_and_replacement_gain() {
        let mut sound = output_state();
        let mut mixer = sound.connect_output().unwrap();
        sound.start(0, 0, 1).unwrap();
        mixer.render(8, &mut [0.0; 2]).unwrap();
        let progress = sound.channels[0].progress.as_ref().unwrap().clone();
        let status = progress.lock().unwrap();
        mixer.render(8, &mut [0.0; 2]).unwrap();
        drop(status);
        sound.control_pair(0, 185, 0).unwrap();
        sound.start(0, 1, 2).unwrap();
        assert!(sound.channel(0).unwrap().is_playing());
        mixer.render(8, &mut [0.0; 8]).unwrap();
        assert!(!sound.channel(0).unwrap().is_playing());
        sound.set_volume(0, 50);
        sound.channels[0].pcm_volume = 100;
        sound.output_replace(0);
        sound.start(0, 1, 3).unwrap();
        let mut output = [0.0; 2];
        mixer.render(8, &mut output).unwrap();
        assert_eq!(output, [0.125; 2]);
        drop(mixer);
        assert!(!sound.channel(0).unwrap().is_playing());
    }

    #[test]
    fn configuration_notification_preserves_playback_and_marks_query_positions() {
        let mut numbers = State::new(0);
        let mut host = CoreHost::new(&mut numbers);
        let mut object = SoundObject::new(true);
        for index in [7, 11, 63] {
            let channel = &mut object.audio.channels[index];
            channel.pcm = Some(Pcm::new(8000, 1, vec![1, 2]).unwrap());
            channel.playback.playing = true;
        }
        object.audio.channels[7].grouped = true;
        object.audio.channels[7].linked = Some(11);
        object.audio.channels[11].playback.playing = false;
        for channel in &mut object.audio.channels {
            channel.allocation_flags = [37, 41];
            channel.ramps.remaining = [17.0, 19.0];
            channel.ramps.active = 3;
        }
        let command = Command {
            target: 1,
            sender: 37,
            code: 19,
            data: [u32::MAX; 3],
        };
        host.host.system_settings = Some(crate::settings::SystemSettings::default());
        for (modern, enabled) in [0, 1, 0x8000_0000]
            .into_iter()
            .flat_map(|modern| [1, 0x8000_0000, 0].map(|enabled| (modern, enabled)))
        {
            host.host.sound_config_enabled = modern;
            host.host.system_settings.as_mut().unwrap().jtalk_enabled = enabled;
            object.audio.channels[0].allocation_flags = [37, 41];
            object.audio.channels[0].ramps.remaining = [17.0, 19.0];
            let mut output = 99;
            let result = object
                .dispatch_command(kind::SOUND, &command, &mut output, &mut host, 1)
                .unwrap();
            assert!(result.done && !result.propagate);
            assert_eq!(output, 1);
            let first = &object.audio.channels[0];
            assert_eq!(
                first.allocation_flags,
                [if enabled == 0 { 1 } else { 37 }, 41]
            );
            assert_eq!(
                first.ramps.remaining,
                [if enabled == 0 { 0.0 } else { 17.0 }, 19.0]
            );
            for channel in &object.audio.channels[1..] {
                assert_eq!(channel.allocation_flags, [37, 41]);
                assert_eq!(channel.ramps.remaining, [17.0, 19.0]);
                assert_eq!(channel.ramps.active, 3);
            }
            assert!(object.audio.channels[7].is_playing());
            assert!(!object.audio.channels[11].is_playing());
        }
        host.host.system_settings = None;
        object.audio.channels[7].grouped = false;
        object.audio.channels[63].grouped = true;
        object.audio.channels[63].linked = Some(63);
        for _ in 0..2 {
            object
                .dispatch_command(kind::SOUND, &command, &mut 99, &mut host, 1)
                .unwrap();
            assert_eq!(object.audio.channels[1].allocation_flags, [1, 41]);
            assert_eq!(object.audio.channels[1].ramps.remaining, [0.0, 19.0]);
            assert!(object.audio.channels[63].is_playing());
        }
        object.base.control.deletion_requested = -1;
        let mut output = 99;
        object
            .dispatch_command(kind::SOUND, &command, &mut output, &mut host, 1)
            .unwrap();
        assert_eq!(output, 99);
        let mut disabled = SoundObject::new(false);
        disabled
            .dispatch_command(kind::SOUND, &command, &mut output, &mut host, 1)
            .unwrap();
        assert_eq!(output, 0);
    }
}
