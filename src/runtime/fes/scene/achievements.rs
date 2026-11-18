use super::Scene;
use crate::{
    Error, Result,
    runtime::{ObjectFactory, Store},
};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_achievement_command(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        let Some(command) = args.first().map(Vec::as_slice) else {
            return Ok(false);
        };
        match command {
            b"steam_init" => self.host.initialize_achievements(owner)?,
            b"steam_achievement_reset" => self.host.reset_achievements(owner)?,
            b"steam_achievement"
            | b"steam_isenable"
            | b"steam_isbroadcast"
            | b"steam_enablescreenshot" => {
                let argument = args
                    .get(1)
                    .map(|value| platform_argument(value, &self.host.script_replacements, offset))
                    .transpose()?;
                match command {
                    b"steam_achievement" => {
                        if let Some(name) = argument {
                            self.host.unlock_achievement(owner, &name)?;
                        }
                    }
                    b"steam_enablescreenshot" => {
                        let value = argument
                            .map_or(Ok(1), |value| scene.interpreter.eval(&value, self.numbers))?;
                        self.host.hook_achievement_screenshots(owner, value == 0)?;
                    }
                    _ => {
                        if let Some(argument) = argument {
                            let destination = scene.interpreter.eval(&argument, self.numbers)?;
                            let value = if command == b"steam_isbroadcast" {
                                self.host.achievement_broadcast(owner)?
                            } else {
                                self.host.achievement_enabled().unwrap_or(0)
                            };
                            if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
                                *slot = value as i32;
                            }
                        }
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

fn platform_argument(
    value: &[u8],
    replacements: &std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    offset: usize,
) -> Result<Vec<u8>> {
    fn before_zero(value: &[u8]) -> &[u8] {
        &value[..value
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(value.len())]
    }
    let mut value = before_zero(value).to_vec();
    if value.len() >= 2048 {
        return Err(Error::new(offset, "FES 平台参数超过缓冲区上限"));
    }
    for (find, replacement) in replacements {
        let (result, terminators) =
            crate::kcs::replace_script_bytes(&value, before_zero(find), before_zero(replacement))
                .map_err(|error| Error::new(offset, error.message))?;
        if result.len() + terminators > 2560 {
            return Err(Error::new(offset, "FES 平台参数超过替换缓冲区上限"));
        }
        value = result;
    }
    Ok(value)
}
