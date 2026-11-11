use super::{ObjectState, Objects, Scene};
use crate::runtime::{ObjectFactory, Store, kind, route_with_buffers};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result};

impl Objects {
    pub(super) fn dispatch_sound_value_command(
        &mut self,
        interpreter: &mut crate::fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &crate::fes::ObjectReference,
        state: &mut crate::expression::State,
    ) -> Result<bool> {
        let command = args[1].as_slice();
        if command == b"play" && target.bank == -1 {
            return Ok(true);
        }
        let mut number = |index: usize, default| {
            args.get(index).map_or(Ok(default), |bytes| {
                interpreter
                    .eval(bytes, state)
                    .map_err(|error| Error::new(offset + error.offset, error.message))
            })
        };
        let value = if command == b"play" { 0 } else { number(2, 0)? };
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                break;
            };
            let sound = match &mut self.get_mut(handle).unwrap().state {
                ObjectState::Sound(sound) => sound,
                _ => continue,
            };
            if command == b"play" {
                sound.action = 1;
                sound.action_requested = true;
                self.invalidate(handle)?;
                let ObjectState::Sound(sound) = &mut self.get_mut(handle).unwrap().state else {
                    unreachable!()
                };
                if args.get(2).is_some() {
                    let value = number(2, 0)?;
                    sound.play_value_changed |= sound.play_parameters[0] != value;
                    sound.play_parameters[0] = value;
                }
                sound.play_parameters[1] = number(3, 1)?;
                sound.play_parameters[2] = number(4, 0)?;
            } else {
                let field = if command == b"channel" {
                    &mut sound.channel
                } else {
                    &mut sound.sound_type
                };
                if *field != value {
                    *field = value;
                    self.invalidate(handle)?;
                }
            }
        }
        Ok(true)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn refresh_fes_sound_configuration(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
            return Ok(());
        };
        sound.dirty = true;
        sound.play_value_changed = false;
        if sound.play_parameters[0] != 0 {
            return Ok(());
        }
        let allowed = self.fes_sound_allowed(sound, false)?;
        let channel = sound.channel as u32;
        let task = self.fes_sound_task(owner, objects)?;
        let playing = self.sound_command(owner, task, 186, [channel, 0, 0], None)?;
        if playing == 0 || !allowed {
            if let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state {
                sound.action_requested = true;
            }
        }
        Ok(())
    }

    pub(super) fn update_fes_sound(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        if self.update_fes_sound_media(owner, objects, handle)? {
            return Err(Error::new(0, "FES SOUND PLAY 处理器需要所属场景"));
        }
        Ok(())
    }

    pub(super) fn update_fes_scene_sound(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        handle: u32,
    ) -> Result<()> {
        self.restore_fes_draw_submission(owner, &mut scene.objects)?;
        if !self.update_fes_sound_media(owner, &mut scene.objects, handle)? {
            return Ok(());
        }
        let object = scene.objects.get(handle).unwrap();
        let ObjectState::Sound(sound) = &object.state else {
            unreachable!()
        };
        if sound.play_handler == Some(false) {
            let name = object.name.clone();
            let bank = object.bank as i32;
            if !sound.event_alias.is_empty() {
                return Err(Error::new(0, "FES SOUND 非空媒体别名尚未接入"));
            }
            let dispatched = scene
                .interpreter
                .dispatch_event_at_host_boundary(&name, bank, "PLAY", None)?;
            if let ObjectState::Sound(sound) = &mut scene.objects.get_mut(handle).unwrap().state {
                sound.play_handler = Some(dispatched);
            }
            if !dispatched {
                return self.finish_fes_sound_action(owner, &mut scene.objects, handle);
            }
        }
        crate::runtime::drain(self.budget, self.limit, || self.run_fes_handler(scene))?;
        self.finish_fes_sound_action(owner, &mut scene.objects, handle)
    }

    fn update_fes_sound_media(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<bool> {
        let ObjectState::Sound(sound) = &objects.get(handle).unwrap().state else {
            unreachable!()
        };
        if sound.play_handler.is_some() {
            return Ok(true);
        }
        if sound.needs_load() {
            let channel = sound.channel as u32;
            let parameters = sound.load_parameters;
            let group = sound.group.clone();
            let mut record = [0; 0x124];
            let has_file = !sound.file.is_empty();
            if has_file {
                let name = self
                    .resources
                    .services
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置 FES 声音资源服务"))?
                    .resolve(b"sound", &sound.file)?;
                let name = &name[..name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len())];
                if name.len() >= 260 {
                    return Err(Error::new(0, "FES 声音补全名称超过存储上限"));
                }
                record[12..12 + name.len()].copy_from_slice(name);
            }
            let task = self.fes_sound_task(owner, objects)?;
            if !has_file {
                self.sound_command(owner, task, 182, [channel, 0, 0], None)?;
            } else {
                if channel != u32::MAX && parameters[0] == 0 {
                    self.sound_command(owner, task, 182, [channel, 0, 0], None)?;
                }
                record[..4].copy_from_slice(&channel.to_le_bytes());
                record[0x110..0x114].copy_from_slice(&1u32.to_le_bytes());
                record[0x118..0x11c].copy_from_slice(&parameters[0].to_le_bytes());
                record[0x11c..0x120].copy_from_slice(&parameters[1].to_le_bytes());
                let channel = if group.is_empty() {
                    self.sound_command(owner, task, 180, [0; 3], Some(&mut record))?;
                    channel
                } else {
                    let mut group = group;
                    group.push(0);
                    let group = self.sound_command(owner, task, 210, [0; 3], Some(&mut group))?;
                    record[4..8].copy_from_slice(&group.to_le_bytes());
                    let channel =
                        self.sound_command(owner, task, 206, [0; 3], Some(&mut record))?;
                    let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state
                    else {
                        unreachable!()
                    };
                    if sound.channel != channel as i32 {
                        sound.channel = channel as i32;
                        objects.invalidate(handle)?;
                    }
                    channel
                };
                let result = self.sound_command(owner, task, 201, [channel, 0, 0], None)?;
                let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
                    unreachable!()
                };
                sound.channel_result = result;
            }
            let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
                unreachable!()
            };
            sound.load_requested = false;
            sound.media_refresh = true;
        }
        let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        let allowed = if sound.action_requested {
            sound.play_value_changed = false;
            let channel = sound.channel as u32;
            let stop = sound.action == 0
                || (sound.play_parameters[0] == 0 && !self.fes_sound_allowed(sound, false)?);
            if stop {
                let task = self.fes_sound_task(owner, objects)?;
                if self.sound_command(owner, task, 186, [channel, 0, 0], None)? != 0 {
                    self.sound_command(owner, task, 185, [channel, 0, 0], None)?;
                }
            }
            let ObjectState::Sound(sound) = &objects.get(handle).unwrap().state else {
                unreachable!()
            };
            self.fes_sound_allowed(sound, sound.sound_type != 0)?
        } else {
            false
        };
        let ObjectState::Sound(sound) = &objects.get(handle).unwrap().state else {
            unreachable!()
        };
        if (sound.is_dirty() || sound.media_refresh || self.host.sound_settings_dirty != 0)
            && !sound.file.is_empty()
        {
            let channel = sound.channel as u32;
            let pan = sound.pan as u32;
            let volume = self.fes_sound_volume(sound)? as u32;
            let task = self.fes_sound_task(owner, objects)?;
            self.sound_command(owner, task, 187, [channel, volume, 0], None)?;
            self.sound_command(owner, task, 190, [channel, pan, 0], None)?;
        }
        let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        sound.media_refresh = false;
        sound.dirty = false;
        if sound.action_requested {
            let channel = sound.channel as u32;
            let count = sound.play_parameters[0] as u32;
            let playing = sound.action != 0;
            let enabled = sound.enabled != 0;
            let task = self.fes_sound_task(owner, objects)?;
            if playing && allowed {
                self.sound_command(
                    owner,
                    task,
                    if enabled { 183 } else { 218 },
                    [channel, if enabled { count } else { 0 }, 0],
                    None,
                )?;
                let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
                    unreachable!()
                };
                if sound.started_tick == u32::MAX {
                    sound.started_tick = self.host.system_time_ms;
                }
                sound.play_handler = Some(false);
                return Ok(true);
            }
            if playing {
                self.sound_command(owner, task, 185, [channel, 0, 0], None)?;
                self.sound_command(owner, task, 218, [channel, 0, 0], None)?;
                let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
                    unreachable!()
                };
                if sound.started_tick == u32::MAX {
                    sound.started_tick = self.host.system_time_ms;
                }
            } else {
                self.sound_command(owner, task, 185, [channel, 0, 0], None)?;
            }
            return self
                .finish_fes_sound_action(owner, objects, handle)
                .map(|()| false);
        }
        self.finish_fes_sound_update(owner, objects, handle)?;
        Ok(false)
    }

    fn finish_fes_sound_action(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        sound.play_handler = None;
        sound.action_requested = false;
        self.finish_fes_sound_update(owner, objects, handle)
    }

    fn finish_fes_sound_update(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let ObjectState::Sound(sound) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        if sound.action == 0 {
            return Ok(());
        }
        if sound.channel == -1 {
            sound.action = 0;
            sound.action_requested = true;
        } else if sound.play_value_changed {
            sound.play_value_changed = false;
            let data = [sound.channel as u32, sound.play_parameters[0] as u32, 0];
            let task = self.fes_sound_task(owner, objects)?;
            self.sound_command(owner, task, 219, data, None)?;
        }
        Ok(())
    }

    fn fes_sound_allowed(
        &self,
        sound: &crate::fes::SoundState,
        system_voice: bool,
    ) -> Result<bool> {
        if self.host.sound_config_enabled == 0 {
            return Err(Error::new(0, "FES SOUND 旧声音配置的播放动作尚未接入"));
        }
        let config = self
            .host
            .sound_config
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未提供新声音分类配置"))?;
        let policy = if system_voice {
            config.file_system_voice(&sound.file)?
        } else {
            config.file_play_enabled(&sound.file)?
        };
        Ok(policy | sound.play_parameters[2] != 0)
    }

    fn fes_sound_volume(&self, sound: &crate::fes::SoundState) -> Result<i32> {
        if self.host.sound_config_enabled == 0 {
            return Err(Error::new(0, "FES SOUND 旧声音配置的播放属性更新尚未接入"));
        }
        let config = self
            .host
            .sound_config
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未提供新声音分类配置"))?;
        let mut volume = config
            .master_volume
            .ok_or_else(|| Error::new(0, "运行时未提供新声音配置的当前主音量"))?;
        if config.master_mute() != 0 {
            return Ok(0);
        }
        if sound.object_type == 0 {
            if !sound.group.is_empty() {
                volume = volume.wrapping_mul(config.group_volume(&sound.group)?) / 100;
            }
            volume = volume.wrapping_mul(config.file_volume(&sound.file)?) / 100;
            if config.file_mute(&sound.file)? != 0 {
                volume = 0;
            }
        }
        Ok(volume.wrapping_mul(sound.volume) / 100)
    }

    fn fes_sound_task(&mut self, owner: u32, objects: &mut Objects) -> Result<u32> {
        let task = match objects.sound_task {
            Some(task) => task,
            None => {
                let task = self.sound_command(owner, 0, 35, [0; 3], None)?;
                objects.sound_task = Some(task);
                task
            }
        };
        if !self
            .tree
            .node(task)
            .is_some_and(|node| node.kind == kind::SOUND)
        {
            return Err(Error::new(0, "FES 声音任务绑定失效"));
        }
        Ok(task)
    }

    fn sound_command(
        &mut self,
        owner: u32,
        task: u32,
        code: u32,
        data: [u32; 3],
        bytes: Option<&mut [u8]>,
    ) -> Result<u32> {
        route_with_buffers(
            self.reborrow(),
            owner,
            Command {
                target: task,
                sender: owner,
                code,
                data,
            },
            &mut CommandBuffers {
                data0: None,
                data1: bytes,
                external: None,
            },
        )
    }

    pub(super) fn fes_sound_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        if args.first().is_some_and(|name| name == b"getsndconfex") {
            return self.fes_sound_config_command(scene, args, offset);
        }
        if args.first().is_none_or(|name| name != b"sysvoiceex") {
            return Ok(false);
        }
        let Some(output) = args.get(1) else {
            return Ok(true);
        };
        if self.host.sound_config_enabled != 0 && self.host.sound_config.is_none() {
            return Err(Error::new(offset, "运行时未提供新声音分类配置"));
        }
        let output = scene.interpreter.eval(output, self.numbers)?;
        let value = if self.host.sound_config_enabled == 0 {
            -1
        } else {
            let mut paths = Vec::new();
            for input in &args[2..] {
                let mut path = Vec::new();
                for component in input.split(|&byte| byte == b',') {
                    path.push(if component.is_empty() {
                        0
                    } else {
                        scene.interpreter.eval(component, self.numbers)?
                    });
                }
                paths.push(path);
            }
            let candidates = self
                .host
                .sound_config
                .as_ref()
                .unwrap()
                .system_voice_candidates(&paths)?;
            if candidates.is_empty() {
                -1
            } else {
                let index = self.host.random.next_integer() as usize % candidates.len();
                candidates[index]
            }
        };
        if let Some(slot) = self.numbers.shared.get_mut(output as usize) {
            *slot = value;
        }
        Ok(true)
    }

    fn fes_sound_config_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        let Some(output) = args.get(1) else {
            return Ok(true);
        };
        let Some(operation) = args.get(2) else {
            scene.interpreter.eval(output, self.numbers)?;
            return Ok(true);
        };
        if !matches!(
            operation.as_slice(),
            b"volume_id" | b"sysplay_id" | b"play_id" | b"volume" | b"voicebgm"
        ) {
            return Err(Error::new(offset, "FES 此声音配置查询尚未接入"));
        }
        if matches!(operation.as_slice(), b"volume" | b"voicebgm") {
            let Some(name) = args.get(3).filter(|name| !name.is_empty()) else {
                scene.interpreter.eval(output, self.numbers)?;
                return Ok(true);
            };
            let config = self
                .host
                .sound_config
                .as_ref()
                .ok_or_else(|| Error::new(offset, "运行时未提供新声音分类配置"))?;
            let value = if operation == b"volume" {
                config.volume_by_name(name)
            } else {
                config.group_voice_volume(name)
            }
            .map_err(|error| Error::new(offset + error.offset, error.message))?;
            let output = scene.interpreter.eval(output, self.numbers)?;
            if let Some(slot) = self.numbers.shared.get_mut(output as usize) {
                *slot = value;
            }
            return Ok(true);
        }
        if self.host.sound_config.is_none() {
            return Err(Error::new(offset, "运行时未提供新声音分类配置"));
        }
        let output = scene.interpreter.eval(output, self.numbers)?;
        let mut path = Vec::new();
        for input in &args[3..] {
            path.push(scene.interpreter.eval(input, self.numbers)?);
        }
        let config = self.host.sound_config.as_ref().unwrap();
        let value = match operation.as_slice() {
            b"volume_id" => config.volume(&path)?,
            b"play_id" => config.play_enabled(&path)?,
            _ => config.system_voice_enabled(&path)?,
        };
        if let Some(slot) = self.numbers.shared.get_mut(output as usize) {
            *slot = value;
        }
        Ok(true)
    }
}
