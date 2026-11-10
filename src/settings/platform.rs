use super::SystemSettings;
use crate::{
    Error, Result,
    language::LanguageState,
    resource::{XmlDocument, XmlWriter},
};

/// 平台用户配置的原始字段；设备应用由宿主负责，目录缺省沿用宿主选择。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformSettings {
    pub window_size: [u16; 2],
    pub fullscreen_size: [u16; 2],
    pub window_fps: u32,
    pub window_mode: u32,
    pub window_bpp: u32,
    pub window_full_wide: u32,
    pub window_limit: u32,
    pub texture_bpp: u32,
    pub texture_min: u32,
    pub texture_mag: u32,
    pub texture_dither: u32,
    pub sound_ds3d: u32,
    pub sound_sticky: u32,
    pub sound_stereo: u32,
    pub window_cursor_mode: u32,
    pub window_screensaver: u32,
    pub window_camouflage: u32,
    pub window_out_info: u32,
    pub etc_cache: u32,
    pub etc_imgcache: u32,
    pub etc_dualcore: u32,
    pub etc_disablemovie: u32,
    pub depend_gdi_cap: u32,
    pub depend_z32: u32,
    pub depend_use_z: u32,
    pub d3d_backbuffer: u32,
    pub d3d_hardware_tnl: u32,
    pub d3d_light: u32,
    pub d3d_vsync: u32,
    pub d3d_antialias: u32,
    pub d3d_fogtype: u32,
    pub d3d_tex_size: u32,
    pub d3d_tex_comp: u32,
    pub window_setmode: u32,
    pub full_screen_full_size: u32,
    pub full_screen_full_pos: u32,
    pub full_screen_wall: u32,
    pub etc_disablepixelshader: u32,
    pub etc_waitd3dcommand: u32,
    pub full_screen_no_full_res: u32,
    pub config_type: u32,
    pub joypad_enabled: u32,
    /// 图片、导出、图像导入目录；None 不创建字段。
    pub directories: [Option<Vec<u16>>; 3],
    pub tts_id: Vec<u16>,
    /// TTS speed、tone 的 float 位模式，保留 NaN 与负零。
    pub tts_parameters: [u32; 2],
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            window_size: [800, 600],
            fullscreen_size: [800, 600],
            window_fps: 60,
            window_mode: 0,
            window_bpp: u32::MAX,
            window_full_wide: 0,
            window_limit: 1,
            texture_bpp: 32,
            texture_min: 0,
            texture_mag: 0,
            texture_dither: 1,
            sound_ds3d: 0,
            sound_sticky: 1,
            sound_stereo: 1,
            window_cursor_mode: 1,
            window_screensaver: 0,
            window_camouflage: 0,
            window_out_info: 1,
            etc_cache: 1,
            etc_imgcache: 32,
            etc_dualcore: 1,
            etc_disablemovie: 0,
            depend_gdi_cap: 0,
            depend_z32: 0,
            depend_use_z: 1,
            d3d_backbuffer: 1,
            d3d_hardware_tnl: 1,
            d3d_light: 1,
            d3d_vsync: 1,
            d3d_antialias: 0,
            d3d_fogtype: 1,
            d3d_tex_size: 0,
            d3d_tex_comp: 0,
            window_setmode: 0,
            full_screen_full_size: 0,
            full_screen_full_pos: 0,
            full_screen_wall: 1,
            etc_disablepixelshader: 0,
            etc_waitd3dcommand: 0,
            full_screen_no_full_res: 0,
            config_type: 0,
            joypad_enabled: 0,
            directories: [None, None, None],
            tts_id: Vec::new(),
            tts_parameters: [1.0f32.to_bits(), 0.0f32.to_bits()],
        }
    }
}

impl PlatformSettings {
    /// 读取启动文档的 SCREEN、GENERAL、APP 等平台字段。
    /// 全屏候选先取低十六位，再交由宿主按可用显示模式调整。
    pub fn apply_startup_xml(
        &mut self,
        input: &[u8],
        mut adjust_fullscreen: impl FnMut([u32; 2]) -> [u32; 2],
    ) -> Result<()> {
        let xml = XmlDocument::parse(input)?;
        self.window_screensaver = xml.integer(b"GENERAL/screensaver", self.window_screensaver)?;
        self.window_camouflage = xml.integer(b"GENERAL/camouflage", self.window_camouflage)?;
        for (axis, value) in ["width", "height"].into_iter().zip(&mut self.window_size) {
            *value = xml.integer(format!("SCREEN/{axis}").as_bytes(), u32::from(*value))? as u16;
        }
        let mut candidate = self.window_size.map(u32::from);
        for (axis, value) in ["full_width", "full_height"]
            .into_iter()
            .zip(&mut candidate)
        {
            *value = u32::from(xml.integer(format!("SCREEN/{axis}").as_bytes(), *value)? as u16);
        }
        self.fullscreen_size = adjust_fullscreen(candidate).map(|value| value as u16);
        for (path, value) in [
            (b"SCREEN/limit".as_slice(), &mut self.window_limit),
            (b"SCREEN/fps", &mut self.window_fps),
            (b"TEXTURE/bpp", &mut self.texture_bpp),
            (b"TEXTURE/filter_min", &mut self.texture_min),
            (b"TEXTURE/filter_mag", &mut self.texture_mag),
            (b"TEXTURE/dither", &mut self.texture_dither),
            (b"SOUND/use3d", &mut self.sound_ds3d),
            (b"SOUND/sticky", &mut self.sound_sticky),
            (b"APP/imgcache", &mut self.etc_imgcache),
            (b"APP/dualcore", &mut self.etc_dualcore),
            (b"SCREEN/full_size", &mut self.full_screen_full_size),
            (b"SCREEN/full_pos", &mut self.full_screen_full_pos),
            (b"SCREEN/wall", &mut self.full_screen_wall),
            (b"SCREEN/no_full_res", &mut self.full_screen_no_full_res),
            (b"CURSOR/mode", &mut self.window_cursor_mode),
        ] {
            *value = xml.integer(path, *value)?;
        }
        self.etc_cache = xml.integer(b"ETC/cache", 1)?;
        Ok(())
    }

    /// 覆盖用户配置；全屏候选尺寸只有被宿主接受才写入，否则保留原值。
    /// 候选缺省取覆盖后的窗口尺寸，检查在截取低十六位之前进行。
    pub fn apply_user_xml(
        &mut self,
        input: &[u8],
        mut supports_fullscreen: impl FnMut([u32; 2]) -> bool,
    ) -> Result<()> {
        let xml = XmlDocument::parse(input)?;
        for (axis, value) in ["width", "height"].into_iter().zip(&mut self.window_size) {
            *value = xml.integer(format!("WINDOW/{axis}").as_bytes(), u32::from(*value))? as u16;
        }
        let mut candidate = self.window_size.map(u32::from);
        for (axis, value) in ["full_width", "full_height"]
            .into_iter()
            .zip(&mut candidate)
        {
            *value = xml.integer(format!("WINDOW/{axis}").as_bytes(), *value)?;
        }
        if supports_fullscreen(candidate) {
            self.fullscreen_size = candidate.map(|value| value as u16);
        }
        self.window_fps = xml.integer(b"WINDOW/fps", self.window_fps)?;
        self.window_mode = xml.integer(b"WINDOW/mode", self.window_mode)?;
        self.window_bpp = xml.integer(b"WINDOW/bpp", self.window_bpp)?;
        self.window_limit = xml.integer(b"WINDOW/limit", self.window_limit)?;
        self.texture_bpp = xml.integer(b"TEXTURE/bpp", self.texture_bpp)?;
        self.texture_min = xml.integer(b"TEXTURE/min", self.texture_min)?;
        self.texture_mag = xml.integer(b"TEXTURE/mag", self.texture_mag)?;
        self.texture_dither = xml.integer(b"TEXTURE/dither", self.texture_dither)?;
        self.sound_ds3d = xml.integer(b"SOUND/ds3d", self.sound_ds3d)?;
        self.sound_sticky = xml.integer(b"SOUND/sticky", self.sound_sticky)?;
        self.sound_stereo = xml.integer(b"SOUND/stereo", self.sound_stereo)?;
        self.window_cursor_mode = xml.integer(b"WINDOW/cursor_mode", self.window_cursor_mode)?;
        self.window_screensaver = xml.integer(b"WINDOW/screensaver", self.window_screensaver)?;
        self.window_camouflage = xml.integer(b"WINDOW/camouflage", self.window_camouflage)?;
        self.window_out_info = xml.integer(b"WINDOW/out_info", self.window_out_info)?;
        self.etc_imgcache = xml.integer(b"ETC/imgcache", self.etc_imgcache)?;
        self.etc_dualcore = xml.integer(b"ETC/dualcore", self.etc_dualcore)?;
        self.etc_disablemovie = xml.integer(b"ETC/disablemovie", 0)?;
        self.depend_gdi_cap = xml.integer(b"DEPEND/gdi_cap", self.depend_gdi_cap)?;
        self.d3d_backbuffer = xml.integer(b"D3D/backbuffer", self.d3d_backbuffer)?;
        self.d3d_hardware_tnl = xml.integer(b"D3D/hardTnL", self.d3d_hardware_tnl)?;
        self.d3d_light = xml.integer(b"D3D/light", self.d3d_light)?;
        self.d3d_vsync = xml.integer(b"D3D/vsync", self.d3d_vsync)?;
        self.d3d_antialias = xml.integer(b"D3D/antialias", self.d3d_antialias)?;
        self.d3d_fogtype = xml.integer(b"D3D/fogtype", self.d3d_fogtype)?;
        self.d3d_tex_size = xml.integer(b"D3D/tex_size", self.d3d_tex_size)?;
        self.d3d_tex_comp = xml.integer(b"D3D/tex_comp", self.d3d_tex_comp)?;
        self.window_setmode = xml.integer(b"WINDOW/setmode", self.window_setmode)?;
        self.full_screen_full_size =
            xml.integer(b"FULL_SCREEN/full_size", self.full_screen_full_size)?;
        self.full_screen_full_pos =
            xml.integer(b"FULL_SCREEN/full_pos", self.full_screen_full_pos)?;
        self.full_screen_wall = xml.integer(b"FULL_SCREEN/wall", self.full_screen_wall)?;
        self.etc_disablepixelshader =
            xml.integer(b"ETC/disablepixelshader", self.etc_disablepixelshader)?;
        self.etc_waitd3dcommand = xml.integer(b"ETC/waitd3dcommand", self.etc_waitd3dcommand)?;
        self.full_screen_no_full_res =
            xml.integer(b"FULL_SCREEN/no_full_res", self.full_screen_no_full_res)?;
        self.config_type = xml.integer(b"CONFIG/type", self.config_type)?;
        self.joypad_enabled = xml.integer(b"KEY/enable_joypad", self.joypad_enabled)?;
        for (name, value) in ["picture", "export", "imgimport"]
            .into_iter()
            .zip(&mut self.directories)
        {
            if let Some(directory) = xml.wide_text(format!("ETC/{name}").as_bytes())? {
                check_wide(&directory, 260)?;
                *value = Some(directory);
            }
        }
        if let Some(id) = xml.wide_text(b"TTS/id")? {
            check_wide(&id, 64)?;
            self.tts_id = id;
        }
        for (name, value) in ["speed", "tone"].into_iter().zip(&mut self.tts_parameters) {
            *value = xml
                .float(format!("TTS/{name}").as_bytes(), f32::from_bits(*value))?
                .to_bits();
        }
        Ok(())
    }

    pub(super) fn write_main(
        &self,
        xml: &mut XmlWriter,
        system: &SystemSettings,
        languages: &LanguageState,
    ) -> Result<()> {
        for (name, value) in ["width", "height", "full_width", "full_height"]
            .into_iter()
            .zip(self.window_size.into_iter().chain(self.fullscreen_size))
        {
            xml.write_integer(&["WINDOW", name], u32::from(value))?;
        }
        xml.write_integer(&["WINDOW", "fps"], self.window_fps)?;
        xml.write_integer(&["WINDOW", "mode"], self.window_mode)?;
        xml.write_integer(&["WINDOW", "bpp"], self.window_bpp)?;
        xml.write_integer(&["WINDOW", "full_wide"], self.window_full_wide)?;
        xml.write_integer(&["WINDOW", "limit"], self.window_limit)?;
        xml.write_integer(&["TEXTURE", "bpp"], self.texture_bpp)?;
        xml.write_integer(&["TEXTURE", "min"], self.texture_min)?;
        xml.write_integer(&["TEXTURE", "mag"], self.texture_mag)?;
        xml.write_integer(&["TEXTURE", "dither"], self.texture_dither)?;
        xml.write_integer(&["SOUND", "ds3d"], self.sound_ds3d)?;
        xml.write_integer(&["SOUND", "sticky"], self.sound_sticky)?;
        xml.write_integer(&["SOUND", "stereo"], self.sound_stereo)?;
        xml.write_integer(&["WINDOW", "cursor_mode"], self.window_cursor_mode)?;
        xml.write_integer(&["WINDOW", "screensaver"], self.window_screensaver)?;
        xml.write_integer(&["WINDOW", "camouflage"], self.window_camouflage)?;
        xml.write_integer(&["WINDOW", "out_info"], self.window_out_info)?;
        xml.write_integer(&["ETC", "cache"], self.etc_cache)?;
        xml.write_integer(&["ETC", "imgcache"], self.etc_imgcache)?;
        xml.write_integer(&["ETC", "dualcore"], self.etc_dualcore)?;
        xml.write_integer(&["ETC", "disablemovie"], self.etc_disablemovie)?;
        if let Some(value) = &self.directories[0] {
            check_wide(value, 260)?;
            xml.write_wide(&["ETC", "picture"], value)?;
        }
        if let Some(value) = &self.directories[1] {
            check_wide(value, 260)?;
            xml.write_wide(&["ETC", "export"], value)?;
        }
        if let Some(value) = &self.directories[2] {
            check_wide(value, 260)?;
            xml.write_wide(&["ETC", "imgimport"], value)?;
        }
        xml.write_integer(&["DEPEND", "gdi_cap"], self.depend_gdi_cap)?;
        xml.write_integer(&["DEPEND", "z32"], self.depend_z32)?;
        xml.write_integer(&["DEPEND", "use_z"], self.depend_use_z)?;
        xml.write_integer(&["D3D", "backbuffer"], self.d3d_backbuffer)?;
        xml.write_integer(&["D3D", "hardTnL"], self.d3d_hardware_tnl)?;
        xml.write_integer(&["D3D", "light"], self.d3d_light)?;
        xml.write_integer(&["D3D", "vsync"], self.d3d_vsync)?;
        xml.write_integer(&["D3D", "antialias"], self.d3d_antialias)?;
        xml.write_integer(&["D3D", "fogtype"], self.d3d_fogtype)?;
        xml.write_integer(&["D3D", "tex_size"], self.d3d_tex_size)?;
        xml.write_integer(&["D3D", "tex_comp"], self.d3d_tex_comp)?;
        xml.write_integer(&["WINDOW", "fullscreen"], system.fullscreen)?;
        xml.write_integer(&["WINDOW", "setmode"], self.window_setmode)?;
        xml.write_integer(&["FULL_SCREEN", "full_size"], self.full_screen_full_size)?;
        xml.write_integer(&["FULL_SCREEN", "full_pos"], self.full_screen_full_pos)?;
        xml.write_integer(&["FULL_SCREEN", "mw_pos"], system.fullscreen_mw_pos)?;
        xml.write_integer(&["FULL_SCREEN", "wall"], self.full_screen_wall)?;
        xml.write_integer(&["ETC", "disablepixelshader"], self.etc_disablepixelshader)?;
        xml.write_integer(&["ETC", "waitd3dcommand"], self.etc_waitd3dcommand)?;
        xml.write_integer(
            &["FULL_SCREEN", "no_full_res"],
            self.full_screen_no_full_res,
        )?;
        languages.write_xml(xml)?;
        check_wide(&self.tts_id, 64)?;
        xml.write_wide(&["TTS", "id"], &self.tts_id)?;
        xml.write_float(&["TTS", "speed"], f32::from_bits(self.tts_parameters[0]))?;
        xml.write_float(&["TTS", "tone"], f32::from_bits(self.tts_parameters[1]))?;
        Ok(())
    }
}

fn check_wide(value: &[u16], capacity: usize) -> Result<()> {
    if value.iter().take_while(|&&unit| unit != 0).count() >= capacity {
        return Err(Error::new(0, "平台配置文字超出字段容量"));
    }
    Ok(())
}
