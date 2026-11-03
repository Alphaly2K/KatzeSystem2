use super::{ObjectState, Objects, Scene};
use crate::runtime::{ObjectFactory, Store};
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
        if operation != b"volume_id" && operation != b"sysplay_id" && operation != b"volume" {
            return Err(Error::new(offset, "FES 此声音配置查询尚未接入"));
        }
        if operation == b"volume" {
            let Some(name) = args.get(3).filter(|name| !name.is_empty()) else {
                scene.interpreter.eval(output, self.numbers)?;
                return Ok(true);
            };
            let value = self
                .host
                .sound_config
                .as_ref()
                .ok_or_else(|| Error::new(offset, "运行时未提供新声音分类配置"))?
                .volume_by_name(name)
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
        let value = if operation == b"volume_id" {
            config.volume(&path)?
        } else {
            config.system_voice_enabled(&path)?
        };
        if let Some(slot) = self.numbers.shared.get_mut(output as usize) {
            *slot = value;
        }
        Ok(true)
    }
}
