/// 全局配置及普通脚本可查询的变量范围。
///
/// 配置覆盖和脚本写回只修改数据，不触发设备或消息对象更新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSettings {
    pub sound: [u8; 0x15fc],
    pub message: [u8; 0x48],
    pub effects: [u8; 0x20],
    pub window: [u8; 0x0c],
    /// 八项局部变量范围，每项依次为起始编号、数量，保留原始 DWORD。
    pub local_flag_ranges: [[u32; 2]; 8],
    /// 八项全局变量范围，格式同 `local_flag_ranges`。
    pub global_flag_ranges: [[u32; 2]; 8],
    /// `GAME/autoselect` 的原始 DWORD。
    pub auto_select: u32,
    /// 用户设置 `WINDOW/fullscreen` 的原始 DWORD。
    pub fullscreen: u32,
    /// 启动 `SCREEN/mw_pos`、用户 `FULL_SCREEN/mw_pos` 的原始 DWORD。
    pub fullscreen_mw_pos: u32,
    pub texture_size: [u32; 2],
    pub texture_grid: [u32; 2],
    /// 八组面块区域：起始列、起始行、列数、行数；首项全一表示停用。
    pub texture_areas: [[u32; 4]; 8],
    pub texture_32bit_row: u32,
    pub texture_16bit_row: u32,
}

impl Default for SystemSettings {
    fn default() -> Self {
        let mut settings = Self {
            sound: [0; 0x15fc],
            message: [0; 0x48],
            effects: [0; 0x20],
            window: [0; 0x0c],
            local_flag_ranges: [[u32::MAX; 2]; 8],
            global_flag_ranges: [[u32::MAX; 2]; 8],
            auto_select: 0,
            fullscreen: 0,
            fullscreen_mw_pos: 0,
            texture_size: [256; 2],
            texture_grid: [0; 2],
            texture_areas: [[u32::MAX; 4]; 8],
            texture_32bit_row: 0,
            texture_16bit_row: 0,
        };
        settings.local_flag_ranges[0] = [0, 512];
        settings.global_flag_ranges[0] = [512, 512];
        put(&mut settings.sound, 0x10, 1);
        for channel in 0..48 {
            let offset = 20 + channel * 116;
            for (field, value) in [(0, 1), (36, 100), (80, 100)] {
                put(&mut settings.sound, offset + field, value);
            }
        }
        for offset in (0x15d4..0x15e8).step_by(4) {
            put(&mut settings.sound, offset, 100);
        }
        for (block, values) in [
            (
                &mut settings.message[..],
                &[
                    4,
                    4,
                    0,
                    15,
                    5,
                    1,
                    0,
                    1,
                    0,
                    100,
                    0,
                    u32::MAX,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ][..],
            ),
            (&mut settings.effects[..], &[0, 0, 1, 1, 1, 5, 0, 0][..]),
            (&mut settings.window[..], &[u32::MAX, 1, 1][..]),
        ] {
            for (bytes, value) in block.chunks_exact_mut(4).zip(values) {
                bytes.copy_from_slice(&value.to_le_bytes());
            }
        }
        settings
    }
}

impl SystemSettings {
    /// 覆盖启动 XML 的变量范围、自动选择、全屏及纹理布局配置；缺失字段保留。
    pub fn apply_startup_xml(&mut self, input: &[u8]) -> crate::Result<()> {
        let xml = crate::resource::XmlDocument::parse(input)?;
        self.auto_select = xml.integer(b"GAME/autoselect", self.auto_select)?;
        self.fullscreen_mw_pos = xml.integer(b"SCREEN/mw_pos", self.fullscreen_mw_pos)?;
        for (name, values) in [
            ("size", &mut self.texture_size),
            ("vt", &mut self.texture_grid),
        ] {
            for (axis, value) in ["x", "y"].into_iter().zip(values) {
                *value = xml.integer(format!("TEXTURE/{name}/{axis}").as_bytes(), *value)?;
            }
        }
        for (index, area) in self.texture_areas.iter_mut().enumerate() {
            for (name, value) in ["sx", "sy", "dx", "dy"].into_iter().zip(area) {
                *value =
                    xml.integer(format!("TEXTURE/area{index:02}/{name}").as_bytes(), *value)?;
            }
        }
        self.texture_32bit_row = xml.integer(b"TEXTURE/bpp32y", self.texture_32bit_row)?;
        self.texture_16bit_row = xml.integer(b"TEXTURE/bpp16y", self.texture_16bit_row)?;
        for (name, ranges) in [
            ("local", &mut self.local_flag_ranges),
            ("global", &mut self.global_flag_ranges),
        ] {
            for (index, range) in ranges.iter_mut().enumerate() {
                for (field, value) in ["start", "count"].into_iter().zip(range) {
                    *value = xml.integer(
                        format!("GAME/flag/{name}{index}/{field}").as_bytes(),
                        *value,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// 按用户设置 XML 覆盖已存在的配置；缺失字段保留，非法整数前缀取零。
    pub fn apply_xml(&mut self, input: &[u8]) -> crate::Result<()> {
        let xml = crate::resource::XmlDocument::parse(input)?;
        self.auto_select = xml.integer(b"GAME/autoselect", self.auto_select)?;
        self.fullscreen = xml.integer(b"WINDOW/fullscreen", self.fullscreen)?;
        self.fullscreen_mw_pos = xml.integer(b"FULL_SCREEN/mw_pos", self.fullscreen_mw_pos)?;
        apply_words(
            &mut self.sound,
            &[
                (0x15d8, b"VOICE/vol_bgm"),
                (0x15dc, b"VOICE/vol_se"),
                (0x15e0, b"VOICE/vol_voice"),
                (0x15e4, b"VOICE/vol_play"),
                (0x15d4, b"VOICE/vol_master"),
                (0x10, b"VOICE/cont_voice"),
                (0x15e8, b"VOICE/mute_master"),
                (0x15ec, b"VOICE/mute_bgm"),
                (0x15f0, b"VOICE/mute_se"),
                (0x15f4, b"VOICE/mute_voice"),
                (0x15f8, b"VOICE/mute_play"),
            ],
            &xml,
        )?;
        for channel in 0..48 {
            let offset = 20 + channel * 116;
            let value = xml.integer(
                format!("VOICE/play{channel:02}").as_bytes(),
                word(&self.sound, offset),
            )?;
            put(&mut self.sound, offset, value);
        }
        apply_words(
            &mut self.message,
            &[
                (0, b"MESSAGE/speed"),
                (4, b"MESSAGE/auto_p"),
                (12, b"MESSAGE/base"),
                (16, b"MESSAGE/CHAR"),
                (8, b"MESSAGE/ind"),
                (24, b"MESSAGE/cont"),
                (20, b"MESSAGE/still"),
                (28, b"MESSAGE/waitpcm"),
                (32, b"MESSAGE/skip_full"),
                (36, b"MESSAGE/log_limit"),
                (40, b"MESSAGE/enable_read"),
                (44, b"MESSAGE/read_color"),
                (48, b"MESSAGE/skipspeed"),
                (52, b"MESSAGE/contskip"),
                (56, b"MESSAGE/wipe"),
                (60, b"MESSAGE/ctrlmode"),
                (64, b"MESSAGE/autoleft"),
                (68, b"MESSAGE/enable_read_sel"),
            ],
            &xml,
        )?;
        apply_words(
            &mut self.effects,
            &[
                (0, b"EFFECT/speed"),
                (8, b"EFFECT/help"),
                (12, b"EFFECT/auto_c"),
                (16, b"EFFECT/auto_cd"),
                (20, b"EFFECT/auto_cds"),
                (24, b"EFFECT/dramatic"),
                (4, b"EFFECT/skipdraw"),
                (28, b"EFFECT/skipmot"),
            ],
            &xml,
        )?;
        apply_words(
            &mut self.window,
            &[
                (0, b"GAME/wndcolor"),
                (4, b"GAME/enabledrag"),
                (8, b"GAME/drawwnd"),
            ],
            &xml,
        )
    }

    /// 用主音量乘四类音量，保留 DWORD 乘法回绕后除以一百的顺序。
    pub fn refresh_sound_volumes(&mut self) {
        let master = word(&self.sound, 0x15d4);
        for index in 0..4 {
            let volume = word(&self.sound, 0x15d8 + index * 4);
            put(
                &mut self.sound,
                index * 4,
                volume.wrapping_mul(master) / 100,
            );
        }
    }

    pub(crate) fn block_mut(&mut self, index: usize) -> &mut [u8] {
        match index {
            0 => &mut self.sound,
            1 => &mut self.message,
            2 => &mut self.effects,
            3 => &mut self.window,
            _ => unreachable!(),
        }
    }
}

fn apply_words(
    block: &mut [u8],
    fields: &[(usize, &[u8])],
    xml: &crate::resource::XmlDocument,
) -> crate::Result<()> {
    for &(offset, path) in fields {
        put(block, offset, xml.integer(path, word(block, offset))?);
    }
    Ok(())
}

fn word(block: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap())
}

fn put(block: &mut [u8], offset: usize, value: u32) {
    block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
