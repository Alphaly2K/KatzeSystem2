use super::SoundState;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

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
                    self.audio
                        .load(bytes, host.resource_services(), output, command.code)?;
                }
                return Ok(CommandOutcome::finished(false));
            }
            206..=209 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "声音分组装载缺少记录缓冲区"))?;
                self.audio
                    .load(bytes, host.resource_services(), output, command.code)?;
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
                        if sound.source.is_some() {
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
    use crate::audio::{Pcm, playback::Playback};
    use crate::{expression::State, kcs::CoreHost};

    fn output_state() -> SoundState {
        let mut sound = SoundState::default();
        let channel = &mut sound.channels[0];
        let pcm: crate::audio::Source = Pcm::new(8, 1, vec![8192, 16384, 24576, 16384])
            .unwrap()
            .into();
        channel.playback = Playback::new(&pcm, true, 0).unwrap();
        channel.source = Some(pcm);
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
            channel.source = Some(Pcm::new(8000, 1, vec![1, 2]).unwrap().into());
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
