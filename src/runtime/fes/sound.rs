use super::Scene;
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result};

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
        if operation != b"volume_id" && operation != b"sysplay_id" {
            return Err(Error::new(offset, "FES 此声音配置查询尚未接入"));
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
