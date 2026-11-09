use super::Scene;
use crate::{
    Error, Result,
    runtime::{ObjectFactory, Store},
};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_config_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        if args.first().is_none_or(|name| name != b"getconf") {
            return Ok(false);
        }
        let Some(destination) = args.get(1) else {
            return Ok(true);
        };
        let mut eval = |index: usize| match args.get(index) {
            Some(value) if !value.is_empty() => scene.interpreter.eval(value, self.numbers),
            _ => Ok(0),
        };
        let destination = if destination.is_empty() { 0 } else { eval(1)? };
        let index = eval(3)?.wrapping_add(eval(4)?);
        let Some(name) = args.get(2) else {
            return Ok(true);
        };
        let language = match name.as_slice() {
            b"lang_ui" => Some(self.host.languages.primary_index),
            b"lang_text_main" => Some(self.host.languages.message_index),
            b"lang_text_sub" => Some(self.host.languages.subtitle_index),
            _ => None,
        };
        let value = if let Some(value) = language {
            value
        } else {
            let (block, word): (u8, usize) = match name.as_slice() {
                b"messpd" => (0, 0),
                b"autospd" => (0, 1),
                b"autoind" => (0, 2),
                b"autospd_b" => (0, 3),
                b"autospd_c" => (0, 4),
                b"stillread" => (0, 5),
                b"contauto" => (0, 6),
                b"waitpcm" => (0, 7),
                b"fullskip" => (0, 8),
                b"enable_readcol" => (0, 10),
                b"readcol" => (0, 11),
                b"skip_speed" => (0, 12),
                b"contskip" => (0, 13),
                b"strscroll" => (0, 14),
                b"ctrlmode" => (0, 15),
                b"autoleft" => (0, 16),
                b"enable_readselcol" => (0, 17),
                b"effspd" => (1, 0),
                b"effcut" => (1, 1),
                b"autocur" => (1, 3),
                b"autodel" => (1, 4),
                b"dorama" => (1, 6),
                b"motcut" => (1, 7),
                b"wndcolor" => (2, 0),
                b"dragwnd" => (2, 1),
                b"wndframe" => (2, 2),
                b"vol_master" => (3, 0x15d4 / 4),
                b"vol_bgm" => (3, 0x15d8 / 4),
                b"vol_se" => (3, 0x15dc / 4),
                b"vol_voice" => (3, 0x15e0 / 4),
                b"vol_play" => (3, 0x15e4 / 4),
                b"mute_master" => (3, 0x15e8 / 4),
                b"mute_bgm" => (3, 0x15ec / 4),
                b"mute_se" => (3, 0x15f0 / 4),
                b"mute_voice" => (3, 0x15f4 / 4),
                b"mute_play" => (3, 0x15f8 / 4),
                b"play" if (index as u32) < 48 => (3, (20 + index as usize * 116) / 4),
                b"play" => {
                    if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
                        *slot = 0;
                    }
                    return Ok(true);
                }
                b"contvoice" => (3, 4),
                b"enable_sceneanm" => (4, 0),
                b"sysplay" | b"fshadow" | b"screen" | b"iswindow" | b"font" | b"ruby"
                | b"fontweight" | b"fontfix" | b"fontprop" | b"activewindow" | b"use_sceneskip"
                | b"use_jtalk" => {
                    return Err(Error::new(offset, "FES 配置查询尚未接入"));
                }
                _ => return Ok(true),
            };
            let settings = self
                .host
                .system_settings
                .as_ref()
                .ok_or_else(|| Error::new(offset, "运行时未配置全局设置"))?;
            let bytes: &[u8] = match block {
                0 => &settings.message,
                1 => &settings.effects,
                2 => &settings.window,
                3 => &settings.sound,
                _ => &settings.scene_animation_enabled.to_le_bytes(),
            };
            u32::from_le_bytes(bytes[word * 4..word * 4 + 4].try_into().unwrap())
        };
        if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
            *slot = value as i32;
        }
        Ok(true)
    }
}
