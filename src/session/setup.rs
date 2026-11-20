use super::{GameRuntime, StartupError, WindowPreferences};
use crate::{
    Error, Result,
    host::{
        audio::SpeechSynthesizer,
        diagnostics::{DiagnosticTrace, NativeTraceEvent},
        effect::EffectServices,
        font::FontServices,
        graphics::GraphicsBackend,
        input::{DeviceBinding, InputDevice},
        movie::MovieDecoderFactory,
        platform::{AchievementServices, PlatformJobServices, SystemServices},
        resources::{ArchiveConfig, ResourceServices},
        storage::{
            ConfigurationServices, ExportServices, FileServices, FolderPaths, SaveServices,
            ScriptSettings,
        },
    },
    runtime::TaskRuntime,
    settings::PlatformSettings,
};

/// Configure a session and bind its host services before starting the game.
///
/// Advancing a frame requires the [`GameRuntime`] returned by [`Self::start_game`].
///
/// ```compile_fail,E0599
/// use katze_system2::session::{FrameTime, GameSetup};
/// let mut setup = GameSetup::new(0, 0).unwrap();
/// setup.advance(FrameTime { tick: 1, increment: 1, system_time_ms: 16 });
/// ```
pub struct GameSetup(TaskRuntime);

impl GameSetup {
    /// Allocate zeroed script storage. Each area is limited to 64 MiB;
    /// `shared_slots` counts 32-bit values, and `external_bytes` counts bytes.
    pub fn new(shared_slots: usize, external_bytes: usize) -> Result<Self> {
        TaskRuntime::prepare(shared_slots, external_bytes).map(Self)
    }

    /// Apply engine settings and platform preferences from the supplied XML.
    /// Returns the game's logical size in pixels, before user window overrides.
    /// The host chooses actual display modes independently.
    /// Invalid input leaves the current settings unchanged.
    pub fn configure_startup(
        &mut self,
        startup: &[u8],
        game_defaults: Option<&[u8]>,
        user_settings: Option<&[u8]>,
    ) -> Result<[u32; 2]> {
        let mut platform = PlatformSettings::default();
        platform.apply_startup_xml(startup, |candidate| candidate)?;
        let logical_size = platform.window_size.map(u32::from);
        if logical_size.contains(&0) {
            return Err(Error::new(0, "启动配置的逻辑尺寸必须非零"));
        }
        if let Some(bytes) = user_settings {
            platform.apply_user_xml(bytes, |_| false)?;
        }
        self.0
            .configure_startup(startup, game_defaults, user_settings)?;
        self.0.set_platform_settings(platform);
        Ok(logical_size)
    }

    /// Effective startup key bindings, including any user overrides.
    pub fn startup_input_bindings(&self) -> Option<&[DeviceBinding; 80]> {
        self.0.startup_input_bindings()
    }

    /// Read the configured window preferences after [`Self::configure_startup`].
    pub fn window_preferences(&self) -> Option<WindowPreferences> {
        WindowPreferences::from_runtime(&self.0)
    }

    /// Set up the canvas, presentation and input coordinates for a logical size
    /// in pixels. Call after applying startup settings. The configured texture
    /// grid, when present, determines the size of the script canvas.
    pub fn configure_canvas(&mut self, size: [u32; 2]) -> Result<()> {
        if size.contains(&0) {
            return Err(Error::new(0, "逻辑画面尺寸必须非零"));
        }
        if size.iter().any(|&axis| axis > i32::MAX as u32) {
            return Err(Error::new(0, "逻辑画面尺寸超过坐标范围"));
        }
        self.0.configure_canvas(size)?;
        self.0.input_coordinates_mut().logical_size = size.map(|axis| axis as i32);
        Ok(())
    }

    /// Bind resources and optional sound definitions before starting the game.
    /// With no sound definition, the session uses the legacy voice definitions
    /// and sound groups in the startup document. Invalid XML leaves the bindings unchanged.
    pub fn set_game_resources(
        &mut self,
        services: impl ResourceServices + 'static,
        startup: &[u8],
        sound_definition: Option<&[u8]>,
        sound_user_settings: Option<&[u8]>,
    ) -> Result<()> {
        self.0
            .set_game_resources(services, startup, sound_definition, sound_user_settings)
    }

    /// Bind a resource service with its own sound routing through the `sound`
    /// category. Use `set_game_resources` to install the game's sound setup.
    pub fn set_resources(&mut self, services: impl ResourceServices + 'static) {
        self.0.set_resources(services);
    }

    pub fn set_archive_config(&mut self, config: ArchiveConfig) {
        self.0.set_archive_config(config);
    }

    pub fn set_folder_paths(&mut self, paths: FolderPaths) {
        self.0.set_folder_paths(paths);
    }

    pub fn set_script_settings(&mut self, settings: ScriptSettings) {
        self.0.set_script_settings(settings);
    }

    /// The last directory selected by the image import dialog, in UTF-16.
    pub fn image_import_directory(&self) -> Option<&[u16]> {
        self.0.platform_settings()?.image_import_directory()
    }

    /// Set the directory for later image import and configuration saves, or
    /// clear it with `None`. Returns false before startup configuration or if
    /// the path contains NUL or exceeds 259 UTF-16 units. Does not access files.
    pub fn set_image_import_directory(&mut self, directory: Option<Vec<u16>>) -> bool {
        self.0
            .platform_settings_mut()
            .is_some_and(|settings| settings.set_image_import_directory(directory))
    }

    pub fn set_input_device(&mut self, device: impl InputDevice + 'static) -> Result<()> {
        self.0.set_input_device(device)
    }

    pub fn set_system_services(&mut self, services: impl SystemServices + 'static) {
        self.0.set_system_services(services);
    }

    pub fn set_achievement_services(&mut self, services: impl AchievementServices + 'static) {
        self.0.set_achievement_services(services);
    }

    pub fn set_file_services(&mut self, services: impl FileServices + 'static) {
        self.0.set_file_services(services);
    }

    pub fn set_export_services(&mut self, services: impl ExportServices + 'static) {
        self.0.set_export_services(services);
    }

    pub fn set_platform_job_services(&mut self, services: impl PlatformJobServices + 'static) {
        self.0.set_platform_job_services(services);
    }

    pub fn set_effect_services(&mut self, services: impl EffectServices + 'static) {
        self.0.set_effect_services(services);
    }

    /// Install optional model playback for FES character planes.
    pub fn set_model_services(
        &mut self,
        services: impl crate::host::model::ModelServices + 'static,
    ) {
        self.0.set_model_services(services);
    }

    pub fn set_speech_synthesizer(&mut self, synthesizer: impl SpeechSynthesizer + 'static) {
        self.0.set_speech_synthesizer(synthesizer);
    }

    pub fn set_configuration_services(&mut self, services: impl ConfigurationServices + 'static) {
        self.0.set_configuration_services(services);
    }

    pub fn set_save_services(&mut self, services: impl SaveServices + 'static) {
        self.0.set_save_services(services);
    }

    pub fn set_fonts(&mut self, fonts: impl FontServices + 'static) {
        self.0.set_fonts(fonts);
    }

    /// Install the renderer for script captures and active screen filter passes.
    /// A backend is required when those passes request readback.
    pub fn set_graphics_backend(&mut self, backend: impl GraphicsBackend + 'static) {
        self.0.set_graphics_backend(backend);
    }

    /// Install the host decoder for movie resources. A decoder is required only
    /// when a script loads a movie. Already loaded movies retain their factory.
    pub fn set_movie_decoder_factory(&mut self, factory: impl MovieDecoderFactory + 'static) {
        self.0.set_movie_decoder_factory(factory);
    }

    /// Load initial translations after configuring startup and binding resources.
    pub fn load_startup_language_tables(&mut self) -> Result<()> {
        self.0.load_startup_language_tables()
    }

    /// Check that resources, input and the canvas are ready. This does not
    /// call host services or attempt to load the entry script.
    pub fn validate_startup(&self) -> std::result::Result<(), StartupError> {
        self.0.validate_session_startup()
    }

    /// Check the startup requirements and create the tasks for the named entry
    /// script. Failure drops the prepared session. No frame is advanced.
    pub fn start_game(self, entry: &[u8]) -> std::result::Result<GameRuntime, StartupError> {
        self.validate_startup()?;
        self.0
            .start_game(entry)
            .map(GameRuntime)
            .map_err(StartupError::StartTasks)
    }

    pub fn set_diagnostic_trace(&mut self, trace: Option<DiagnosticTrace>) {
        self.0.set_diagnostic_trace(trace);
    }

    pub fn set_native_trace(&mut self, trace: impl Fn(NativeTraceEvent) + 'static) {
        self.0.set_native_trace(trace);
    }

    /// Transfer the prepared state to the task API for custom startup or inspection.
    pub fn into_task_runtime(self) -> TaskRuntime {
        self.0
    }
}
