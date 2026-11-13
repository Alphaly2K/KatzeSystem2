use super::{
    LoopPoints, Pcm, SoundChannel, SoundGroups, control::DeferredControl, output::Action,
    playback::Playback,
};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 循环范围、六十四个物理通道及其原始累计时刻。
pub struct SoundState {
    pub(super) loops: LoopPoints,
    pub(super) channels: [SoundChannel; 64],
    pub(super) groups: Option<BTreeMap<u32, GroupRange>>,
    pub(super) output: Option<super::output::Link>,
    pub(super) movie: Option<crate::movie::AudioOutput>,
}

pub(super) struct GroupRange {
    pub(super) id: u32,
    pub(super) range: [i32; 2],
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            loops: LoopPoints::default(),
            channels: std::array::from_fn(|_| SoundChannel::default()),
            groups: None,
            output: None,
            movie: None,
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

    pub(super) fn select_channel(&mut self, range: [i32; 2]) -> Result<u32> {
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
        sound.source = None;
        sound.playback = Playback::default();
        sound.resampler = super::mix::Resampler::default();
        sound.clock.elapsed = 0;
        sound.loop_range[0] = 0;
        sound.allocation_flags = [0; 2];
        sound.deferred_control = None;
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
            .source
            .as_ref()
            .ok_or_else(|| Error::new(0, "声音通道尚未装载 PCM"))?;
        let count = frames
            .checked_mul(pcm.channels() as usize)
            .filter(|count| *count <= read::MAX_BYTES / 2)
            .ok_or_else(|| Error::new(0, "声音采样输出超过容量上限"))?;
        let mut output = vec![0; count];
        sound.resampler = super::mix::Resampler::default();
        sound.playback.read(pcm, &mut output, true)?;
        Pcm::new(pcm.sample_rate(), pcm.channels(), output)
    }

    pub(super) fn update_idle(&mut self, now: u32, stereo: bool) -> Result<()> {
        self.sync_output();
        for index in 0..self.channels.len() {
            let channel = &mut self.channels[index];
            if channel.allocation_flags[0] != 0 || channel.deferred_control.is_some() {
                // 到达零音量后的下一次更新才执行收尾；淡出期间不累计通道时刻。
                if channel.ramps.remaining[0] <= 0.0 {
                    self.finish_deferred(index)?;
                } else {
                    let volume = channel.ramps.advance(0, now);
                    if channel.source.is_some() {
                        channel.pcm_volume = volume;
                    }
                }
                continue;
            }
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
        Ok(())
    }

    fn finish_deferred(&mut self, index: usize) -> Result<()> {
        let sound = &mut self.channels[index];
        let release = sound.allocation_flags[0] != 0;
        let control = sound.deferred_control.take();
        if release {
            sound.unload();
        } else {
            match control {
                Some(DeferredControl::Pause) => {
                    sound.playback.pause();
                    sound.resampler.paused = true;
                }
                Some(DeferredControl::Stop) => {
                    sound.resampler = super::mix::Resampler::default();
                    if let Some(pcm) = &sound.source {
                        sound.playback.stop(pcm)?;
                    }
                }
                None => (),
            }
            sound.started_tick = u32::MAX;
            sound.clock = super::channel::ChannelClock::default();
        }
        sound.ramps.active = 0;
        sound.allocation_flags = [0; 2];
        sound.parameters = [0; 2];
        sound.linked = None;
        sound.pcm_volume = sound.requested_volume as i64 as i32;
        if release {
            self.output_replace(index);
        } else {
            self.output_action(
                index,
                match control {
                    Some(DeferredControl::Pause) => Action::Pause,
                    Some(DeferredControl::Stop) => Action::Stop,
                    None => return Ok(()),
                },
            );
        }
        Ok(())
    }

    pub(super) fn configuration_changed(&mut self) {
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

    pub(super) fn unload(&mut self, channel: u32, now: u32) -> Result<()> {
        let Some(sound) = self.channels.get(channel as usize) else {
            return Ok(());
        };
        let linked = sound.linked;
        for index in [Some(channel), linked].into_iter().flatten() {
            let sound = &mut self.channels[index as usize];
            if sound.grouped {
                if sound.is_playing() && sound.allocation_flags[0] == 0 {
                    sound.allocation_flags[0] = 1;
                    sound.deferred_control = None;
                    sound.begin_fade_out(now);
                }
                sound.started_tick = u32::MAX;
                sound.clock = super::channel::ChannelClock::default();
            } else {
                sound.unload();
                self.output_replace(index as usize);
            }
        }
        Ok(())
    }

    pub(super) fn duration_ms(&self, channel: u32) -> u32 {
        self.channels.get(channel as usize).map_or(0, |sound| {
            self.channels[sound.linked.unwrap_or(channel) as usize].duration_ms()
        })
    }

    pub(super) fn property_channel(&self, channel: u32) -> Option<usize> {
        self.channels
            .get(channel as usize)
            .map(|sound| sound.linked.unwrap_or(channel) as usize)
    }

    pub(super) fn set_volume(&mut self, channel: u32, volume: i32) {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            sound.ramps.active &= !1;
            sound.requested_volume = volume as f32;
            if sound.source.is_some() {
                sound.pcm_volume = volume;
            }
        }
        self.output_gains();
    }

    pub(super) fn set_pan(&mut self, channel: u32, pan: i32, stereo: bool) {
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            sound.ramps.active &= !2;
            if stereo {
                sound.pcm_pan = pan.clamp(-100, 100);
            }
        }
        self.output_gains();
    }

    pub(super) fn start_ramp(
        &mut self,
        channel: u32,
        packed: u32,
        pan: bool,
        now: u32,
    ) -> Result<()> {
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

    pub(super) fn start(&mut self, channel: u32, count: u32, tick: u32) -> Result<()> {
        self.sync_output();
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.parameters[0] != 0 {
                return Err(Error::new(0, "声音淡变播放尚未接入"));
            }
            sound.ramps.active = 0;
            sound.deferred_control = None;
            if let Some(pcm) = &sound.source {
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

    pub(super) fn control_pair(
        &mut self,
        channel: u32,
        command: u32,
        tick: u32,
        now: u32,
    ) -> Result<()> {
        self.sync_output();
        let Some(sound) = self.channels.get(channel as usize) else {
            return Ok(());
        };
        let linked = sound.linked;
        for index in [Some(channel), linked].into_iter().flatten() {
            let sound = &mut self.channels[index as usize];
            if sound.grouped && matches!(command, 184 | 185) {
                sound.allocation_flags[0] = 0;
                sound.deferred_control = Some(if command == 184 {
                    DeferredControl::Pause
                } else {
                    DeferredControl::Stop
                });
                sound.begin_fade_out(now);
                continue;
            }
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
                    if let Some(pcm) = &sound.source {
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

    pub(super) fn is_playing(&self, channel: u32) -> bool {
        self.channels.get(channel as usize).is_some_and(|sound| {
            sound.is_playing()
                || sound
                    .linked
                    .is_some_and(|index| self.channels[index as usize].is_playing())
        })
    }

    pub(super) fn change_count(&mut self, channel: u32, count: u32) -> Result<()> {
        self.sync_output();
        if let Some(index) = self.property_channel(channel) {
            let sound = &mut self.channels[index];
            if sound.source.is_some() {
                let range = playback_range(sound, count);
                sound.playback.change_count(count, range);
                self.output_action(index, Action::Count(count, range));
            }
        }
        Ok(())
    }
}

fn playback_range(sound: &SoundChannel, count: u32) -> [i64; 2] {
    if count == 1 {
        [0, -1]
    } else {
        sound.loop_range
    }
}
