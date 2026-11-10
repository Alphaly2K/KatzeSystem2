use super::{PlatformSettings, SystemSettings, word};
use crate::{Result, input::DeviceKeys, language::LanguageState, resource::XmlWriter};

/// 按平台主配置、用户选项的顺序写入同一文档，包含语言及启用的按键槽。
pub fn write_configuration(
    xml: &mut XmlWriter,
    system: &SystemSettings,
    platform: &PlatformSettings,
    languages: &LanguageState,
    keys: &DeviceKeys,
) -> Result<()> {
    platform.write_main(xml, system, languages)?;
    system.write_options(xml, Some((platform, keys)))
}

pub(super) const SOUND_FIELDS: &[(usize, &str)] = &[
    (0x15d8, "VOICE/vol_bgm"),
    (0x15dc, "VOICE/vol_se"),
    (0x15e0, "VOICE/vol_voice"),
    (0x15e4, "VOICE/vol_play"),
    (0x15d4, "VOICE/vol_master"),
    (0x10, "VOICE/cont_voice"),
    (0x15e8, "VOICE/mute_master"),
    (0x15ec, "VOICE/mute_bgm"),
    (0x15f0, "VOICE/mute_se"),
    (0x15f4, "VOICE/mute_voice"),
    (0x15f8, "VOICE/mute_play"),
];

pub(super) const MESSAGE_FIELDS: &[(usize, &str)] = &[
    (0, "MESSAGE/speed"),
    (4, "MESSAGE/auto_p"),
    (12, "MESSAGE/base"),
    (16, "MESSAGE/CHAR"),
    (8, "MESSAGE/ind"),
    (24, "MESSAGE/cont"),
    (20, "MESSAGE/still"),
    (28, "MESSAGE/waitpcm"),
    (32, "MESSAGE/skip_full"),
    (36, "MESSAGE/log_limit"),
    (60, "MESSAGE/ctrlmode"),
    (64, "MESSAGE/autoleft"),
    (40, "MESSAGE/enable_read"),
    (44, "MESSAGE/read_color"),
    (48, "MESSAGE/skipspeed"),
    (52, "MESSAGE/contskip"),
    (56, "MESSAGE/wipe"),
    (68, "MESSAGE/enable_read_sel"),
];

pub(super) const EFFECTS_FIELDS: &[(usize, &str)] = &[
    (0, "EFFECT/speed"),
    (4, "EFFECT/skipdraw"),
    (8, "EFFECT/help"),
    (12, "EFFECT/auto_c"),
    (16, "EFFECT/auto_cd"),
    (20, "EFFECT/auto_cds"),
    (24, "EFFECT/dramatic"),
    (28, "EFFECT/skipmot"),
];

pub(super) const WINDOW_FIELDS: &[(usize, &str)] = &[
    (0, "GAME/wndcolor"),
    (4, "GAME/enabledrag"),
    (8, "GAME/drawwnd"),
];

impl SystemSettings {
    /// 向同一文档写入已支持的用户设置；未指定的字体名继续继承，不创建对应字段。
    /// 启动专用的变量范围、纹理布局和自动速度预设不写入用户配置。
    pub fn write_xml(&self, xml: &mut XmlWriter) -> Result<()> {
        xml.write_integer(&["WINDOW", "fullscreen"], self.fullscreen)?;
        xml.write_integer(&["FULL_SCREEN", "mw_pos"], self.fullscreen_mw_pos)?;
        self.write_options(xml, None)
    }

    fn write_options(
        &self,
        xml: &mut XmlWriter,
        platform_keys: Option<(&PlatformSettings, &DeviceKeys)>,
    ) -> Result<()> {
        if let Some((platform, _)) = platform_keys {
            xml.write_integer(&["CONFIG", "type"], platform.config_type)?;
        }
        for (name, value) in ["face", "faceruby"].into_iter().zip(&self.font_names) {
            if let Some(value) = value {
                xml.write_wide(&["FONT", name], value)?;
            }
        }
        for (index, path) in [
            (1, &["FONT", "shadow", "rightdown"][..]),
            (0, &["FONT", "shadow", "around"][..]),
            (2, &["FONT", "bold"][..]),
            (3, &["FONT", "proportional"][..]),
            (4, &["FONT", "fixed"][..]),
        ] {
            xml.write_integer(path, self.font_controls[index])?;
        }
        if let Some((platform, keys)) = platform_keys {
            xml.write_integer(&["KEY", "enable_joypad"], platform.joypad_enabled)?;
            keys.write_xml(xml)?;
        }
        write_words(xml, &self.sound, &SOUND_FIELDS[..5])?;
        for channel in 0..48 {
            xml.write_integer(
                &["VOICE", &format!("play{channel:02}")],
                word(&self.sound, 20 + channel * 116),
            )?;
        }
        write_words(xml, &self.sound, &SOUND_FIELDS[5..])?;
        // 其余消息选项在效果、窗口及系统声音之后写入，保留同级字段顺序。
        write_words(xml, &self.message, &MESSAGE_FIELDS[..12])?;
        write_words(xml, &self.effects, EFFECTS_FIELDS)?;
        write_words(xml, &self.window, WINDOW_FIELDS)?;
        for (channel, &value) in self.system_voice_play.iter().enumerate() {
            xml.write_integer(&["SYSTEMVOICE", &format!("play{channel:02}")], value)?;
        }
        write_words(xml, &self.message, &MESSAGE_FIELDS[12..])?;
        for (name, value) in [
            ("autoselect", self.auto_select),
            ("always_active", self.always_active),
            ("enable_scene_anm", self.scene_animation_enabled),
            ("use_scene_skip", self.scene_skip_enabled),
            ("use_jtalk", self.jtalk_enabled),
        ] {
            xml.write_integer(&["GAME", name], value)?;
        }
        Ok(())
    }
}

fn write_words(xml: &mut XmlWriter, block: &[u8], fields: &[(usize, &str)]) -> Result<()> {
    for &(offset, path) in fields {
        xml.write_integer(&path.split('/').collect::<Vec<_>>(), word(block, offset))?;
    }
    Ok(())
}
