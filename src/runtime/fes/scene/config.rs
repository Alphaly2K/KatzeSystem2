use super::Scene;
use crate::runtime::fes::fonts::font_name_bytes;
use crate::text::font_name;
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
        let write = match args.first().map(Vec::as_slice) {
            Some(b"getconf") => false,
            Some(b"setconf") => true,
            _ => return Ok(false),
        };
        let Some(destination) = args.get(1) else {
            return Ok(true);
        };
        let mut eval = |index: usize| match args.get(index) {
            Some(value) if !value.is_empty() => scene.interpreter.eval(value, self.numbers),
            _ => Ok(0),
        };
        let destination = if destination.is_empty() { 0 } else { eval(1)? };
        let first_index = eval(3)?;
        let index = first_index.wrapping_add(eval(4)?);
        let Some(name) = args.get(2) else {
            return Ok(true);
        };
        if write && self.host.system_settings.is_none() {
            return Err(Error::new(offset, "运行时未配置全局设置"));
        }
        if matches!(name.as_slice(), b"font" | b"ruby") {
            let settings = self
                .host
                .system_settings
                .as_mut()
                .ok_or_else(|| Error::new(offset, "运行时未配置全局设置"))?;
            let selected = usize::from(name == b"ruby");
            if write {
                let bytes = self
                    .numbers
                    .strings
                    .get(&(destination as u32))
                    .map_or(&[][..], Vec::as_slice);
                let name = font_name(bytes, offset)?;
                if selected == 0 {
                    self.host
                        .message_defaults
                        .get_or_insert_default()
                        .font_name
                        .clone_from(&name);
                }
                settings.font_names[selected] = Some(name);
                self.finish_fes_config_write(offset)?;
            } else {
                let name = settings.font_names[selected].as_deref().unwrap_or_else(|| {
                    self.host
                        .message_defaults
                        .as_ref()
                        .map_or(&[][..], |defaults| defaults.font_name.as_slice())
                });
                self.numbers
                    .strings
                    .insert(destination as u32, font_name_bytes(name));
            }
            return Ok(true);
        }
        let language = match name.as_slice() {
            b"lang_ui" => Some(self.host.languages.primary_index),
            b"lang_text_main" => Some(self.host.languages.message_index),
            b"lang_text_sub" => Some(self.host.languages.subtitle_index),
            _ => None,
        };
        let value = if let Some(value) = language {
            if write {
                if name != b"lang_text_sub" {
                    return Err(Error::new(offset, "FES 主语言切换通知尚未接入"));
                }
                self.host.languages.subtitle_index =
                    if (destination as u32 as usize) < self.host.languages.prefixes.len() {
                        destination as u32
                    } else {
                        u32::MAX
                    };
                self.finish_fes_config_write(offset)?;
                return Ok(true);
            }
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
                b"sysplay" if (index as u32) < 48 => {
                    if (first_index as u32) >= 48 {
                        return Err(Error::new(offset, "FES 系统声音首索引超出范围"));
                    }
                    (5, first_index as usize)
                }
                b"play" | b"sysplay" => {
                    if write {
                        self.finish_fes_config_write(offset)?;
                    } else if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
                        *slot = 0;
                    }
                    return Ok(true);
                }
                b"contvoice" => (3, 4),
                b"enable_sceneanm" => (4, 0),
                b"activewindow" => (6, 0),
                b"use_sceneskip" => (7, 0),
                b"use_jtalk" => (8, 0),
                b"fshadow" => {
                    if first_index < 0 {
                        return Err(Error::new(offset, "FES 字体阴影索引不能为负值"));
                    }
                    (9, first_index.min(2) as usize)
                }
                b"fontweight" => (9, 2),
                b"fontprop" => (9, 3),
                b"fontfix" => (9, 4),
                b"screen" | b"iswindow" => {
                    return Err(Error::new(
                        offset,
                        if write {
                            "FES 配置写回尚未接入"
                        } else {
                            "FES 配置查询尚未接入"
                        },
                    ));
                }
                _ => {
                    if write {
                        self.finish_fes_config_write(offset)?;
                    }
                    return Ok(true);
                }
            };
            if write {
                let settings = self
                    .host
                    .system_settings
                    .as_mut()
                    .ok_or_else(|| Error::new(offset, "运行时未配置全局设置"))?;
                if name == b"autospd" {
                    settings
                        .select_auto_wait_preset(destination as u32)
                        .map_err(|error| Error::new(offset, error.message))?;
                } else if block == 4 {
                    settings.scene_animation_enabled = destination as u32;
                } else if block == 5 {
                    settings.system_voice_play[word] = destination as u32;
                } else if block == 6 {
                    settings.always_active = destination as u32;
                } else if block == 7 {
                    settings.scene_skip_enabled = destination as u32;
                } else if block == 8 {
                    settings.jtalk_enabled = destination as u32;
                } else if block == 9 {
                    settings.font_controls[word] = destination as u32;
                    settings
                        .apply_message_defaults(self.host.message_defaults.get_or_insert_default());
                } else {
                    let bytes: &mut [u8] = match block {
                        0 => &mut settings.message,
                        1 => &mut settings.effects,
                        2 => &mut settings.window,
                        _ => &mut settings.sound,
                    };
                    bytes[word * 4..word * 4 + 4]
                        .copy_from_slice(&(destination as u32).to_le_bytes());
                }
                self.finish_fes_config_write(offset)?;
                return Ok(true);
            }
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
                4 => &settings.scene_animation_enabled.to_le_bytes(),
                5 => &settings.system_voice_play[word].to_le_bytes(),
                6 => &settings.always_active.to_le_bytes(),
                7 => &settings.scene_skip_enabled.to_le_bytes(),
                8 => &settings.jtalk_enabled.to_le_bytes(),
                _ => &settings.font_controls[word].to_le_bytes(),
            };
            let byte_offset = if block < 4 { word * 4 } else { 0 };
            u32::from_le_bytes(bytes[byte_offset..byte_offset + 4].try_into().unwrap())
        };
        if let Some(slot) = self.numbers.shared.get_mut(destination as usize) {
            *slot = value as i32;
        }
        Ok(true)
    }

    fn finish_fes_config_write(&mut self, offset: usize) -> Result<()> {
        self.host
            .system_settings
            .as_mut()
            .ok_or_else(|| Error::new(offset, "运行时未配置全局设置"))?
            .refresh_sound_volumes();
        self.host.system_settings_dirty = 1;
        Ok(())
    }
}
