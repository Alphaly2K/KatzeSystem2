use super::{INPUT_TASK, ProgramLoader, TaskRuntime};
use crate::{
    Error, Result,
    audio::{SoundConfig, SoundGroups},
    graphics::{DrawingContext, Projection, RenderTarget, TextureFilter},
    host::{
        input::{DeviceBinding, DeviceKeys},
        platform::WindowPreferences,
        resources::{CachedResources, ResourceServices},
    },
    language::{LanguageState, StringTableConfig},
    resource::XmlDocument,
    settings::SystemSettings,
};

impl<L: ProgramLoader> TaskRuntime<L> {
    /// Apply engine settings and input bindings before starting the game. Optional
    /// game defaults override engine settings; user settings override both engine
    /// settings and input bindings. The caller supplies the document bytes and
    /// separately binds resources and devices.
    /// Invalid input leaves the current settings unchanged.
    pub fn configure_startup(
        &mut self,
        startup: &[u8],
        game_defaults: Option<&[u8]>,
        user_settings: Option<&[u8]>,
    ) -> Result<()> {
        if self.tree.next_id() != INPUT_TASK {
            return Err(Error::new(0, "启动配置必须在创建游戏任务前应用"));
        }
        let mut settings = SystemSettings::default();
        settings.apply_startup_xml(startup)?;
        if let Some(bytes) = game_defaults {
            settings.apply_game_xml(bytes)?;
        }
        let mut languages = LanguageState::from_xml(startup)?;
        let mut keys = DeviceKeys::from_xml(startup)?;
        if let Some(bytes) = user_settings {
            settings.apply_xml(bytes)?;
            languages.apply_user_xml(bytes)?;
            keys.apply_user_xml(bytes)?;
        }
        settings.refresh_sound_volumes();

        let xml = XmlDocument::parse(startup)?;
        let unicode_text = xml.integer(b"APP/utfmode", 0)?;
        let message_time_scale = xml.integer(b"GAME/strframe_frame", 0)?;
        let string_table_config = StringTableConfig::from_xml(startup)?;
        let mut message_defaults = Default::default();
        settings.apply_message_defaults(&mut message_defaults);

        self.host.languages = languages;
        self.host.string_table_config = Some(string_table_config);
        self.host.unicode_text = unicode_text;
        self.host.message_time_scale = Some(message_time_scale);
        self.host.message_defaults = Some(message_defaults);
        self.host.system_settings = Some(settings);
        self.host.configuration_keys = Some(keys);
        Ok(())
    }

    /// Startup bindings with user overrides applied. Pass these to the input
    /// device when constructing it. Saving prefers the attached device's current
    /// bindings when available and otherwise uses these bindings.
    pub fn startup_input_bindings(&self) -> Option<&[DeviceBinding; 80]> {
        self.host
            .configuration_keys
            .as_ref()
            .map(DeviceKeys::bindings)
    }

    /// The effective engine settings, if startup settings have been applied.
    pub fn system_settings(&self) -> Option<&SystemSettings> {
        self.host.system_settings.as_ref()
    }

    pub(crate) fn window_preferences(&self) -> Option<WindowPreferences> {
        let platform = self.platform_settings()?;
        let system = self.system_settings()?;
        Some(WindowPreferences {
            window_size: platform.window_size.map(u32::from),
            fullscreen_size: platform.fullscreen_size.map(u32::from),
            fullscreen: system.fullscreen != 0,
        })
    }

    /// Prepare the standard canvas and frame view for a logical surface size in pixels.
    /// Call after applying startup settings. A configured tile grid determines the
    /// canvas size; without a grid the canvas uses the logical surface size.
    /// This replaces the existing canvas on success and preserves it on error.
    pub fn configure_canvas(&mut self, surface_size: [u32; 2]) -> Result<()> {
        let target = match self.system_settings() {
            Some(settings) if !settings.texture_grid.contains(&0) => {
                let [columns, rows] = settings.texture_grid;
                let count = (columns as usize)
                    .checked_mul(rows as usize)
                    .filter(|&count| count <= crate::read::MAX_ITEMS)
                    .ok_or_else(|| Error::new(0, "画布面块数量超过上限"))?;
                let mut target = RenderTarget::from_tiles(
                    settings.texture_size,
                    settings.texture_grid,
                    vec![None; count],
                )?;
                target.prepare_tiles(&settings.texture_areas, settings.texture_16bit_row)?;
                target
            }
            _ => RenderTarget::new(surface_size)?,
        };
        self.set_surface_size(surface_size);
        *self.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(target),
        };
        let projection = Projection::default();
        let parent = projection.matrix;
        self.set_frame_view(projection, parent, TextureFilter::Point);
        Ok(())
    }

    /// Bind the game's resource service and sound configuration before startup.
    /// The service resolves archive categories and opens resources. The runtime
    /// selects the category for each sound name using the supplied definition.
    /// With no definition, it uses the startup voice definitions and sound groups.
    /// Invalid XML leaves both the active configuration and resource service unchanged.
    pub fn set_game_resources(
        &mut self,
        services: impl ResourceServices + 'static,
        startup: &[u8],
        sound_definition: Option<&[u8]>,
        sound_user_settings: Option<&[u8]>,
    ) -> Result<()> {
        if self.tree.next_id() != INPUT_TASK {
            return Err(Error::new(0, "游戏资源必须在创建游戏任务前绑定"));
        }
        let (definition, sound, groups) = if let Some(bytes) = sound_definition {
            let definition = SoundConfig::from_xml(bytes)?;
            let mut sound = definition.clone();
            if let Some(user) = sound_user_settings {
                sound.apply_user_settings_xml(user)?;
            }
            (Some(definition), Some(sound), SoundGroups::from_xml(bytes)?)
        } else {
            (None, None, SoundGroups::from_startup_xml(startup)?)
        };
        self.resources.services = Some(Box::new(
            CachedResources::new(services).with_sound_names(definition, startup)?,
        ));
        self.resources.animation_aliases = None;
        self.host.sound_config_enabled = u32::from(sound.is_some());
        self.host.sound_config = sound;
        self.host.sound_groups = Some(groups);
        Ok(())
    }
}
