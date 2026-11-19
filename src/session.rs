//! Standard game session for embedding CatSystem2. The application supplies
//! [`crate::host`] services and drives the session one frame at a time.

use crate::{Error, Result, host::*, runtime::TaskRuntime};
use std::sync::Arc;

/// Clock values supplied by the host for a single update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTime {
    /// Absolute game clock in logical ticks (normally 60 per second).
    pub tick: u32,
    /// Logical increment forwarded to tasks for this update.
    pub increment: u32,
    /// Wrapping millisecond clock for system queries and movies.
    pub system_time_ms: u32,
}

/// The outcome of advancing a session by one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    Running {
        /// The host can read the current string tables to refresh its UI.
        interface_language_refreshed: bool,
    },
    /// The root task has exited. The host should stop advancing and presenting.
    Exited,
}

/// A game session backed by resources supplied by the host. Prepare it,
/// configure it and bind services, then start the entry script.
pub struct GameRuntime(TaskRuntime);

impl GameRuntime {
    /// Allocate zeroed script storage. Each area is limited to 64 MiB;
    /// `shared_slots` counts 32-bit values, and `external_bytes` counts bytes.
    pub fn prepare(shared_slots: usize, external_bytes: usize) -> Result<Self> {
        TaskRuntime::prepare(shared_slots, external_bytes).map(Self)
    }

    /// Apply engine settings and platform preferences from the supplied XML.
    /// Returns the game's logical size in pixels, before user window overrides.
    /// The host chooses actual display modes and may replace the platform settings.
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
        self.set_platform_settings(platform);
        Ok(logical_size)
    }

    /// Effective startup key bindings, including any user overrides.
    pub fn startup_input_bindings(&self) -> Option<&[DeviceBinding; 80]> {
        self.0.startup_input_bindings()
    }

    pub fn system_settings(&self) -> Option<&SystemSettings> {
        self.0.system_settings()
    }

    /// Set up the canvas, presentation and input coordinates for a logical size
    /// in pixels. Call after applying startup settings. The configured texture
    /// grid, when present, determines the size of the script canvas.
    pub fn configure_canvas(&mut self, size: [u32; 2]) -> Result<()> {
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

    pub fn set_platform_settings(&mut self, settings: PlatformSettings) {
        self.0.set_platform_settings(settings);
    }

    pub fn platform_settings(&self) -> Option<&PlatformSettings> {
        self.0.platform_settings()
    }

    pub fn platform_settings_mut(&mut self) -> Option<&mut PlatformSettings> {
        self.0.platform_settings_mut()
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

    /// Install the executor for script offscreen drawing and readback. The
    /// default uses the software backend. The host handles final presentation.
    pub fn set_graphics_backend(&mut self, backend: impl GraphicsBackend + 'static) {
        self.0.set_graphics_backend(backend);
    }

    /// Load initial translations after configuring startup and binding resources.
    pub fn load_startup_language_tables(&mut self) -> Result<()> {
        self.0.load_startup_language_tables()
    }

    pub fn string_tables(&self) -> &StringTables {
        self.0.string_tables()
    }

    /// Start the named entry script. Failure drops the prepared session and any
    /// tasks it created during startup. No frame is advanced by this call.
    pub fn start_game(self, entry: &[u8]) -> Result<Self> {
        self.0.start_game(entry).map(Self)
    }

    /// Advance one frame, applying pending language changes first. On a
    /// language error, the failed request and remaining requests can be retried.
    pub fn advance(&mut self, time: FrameTime) -> Result<FrameOutcome> {
        self.0.advance(time)
    }

    /// Borrow the current draw list and canvas for host presentation. If a
    /// capture token is supplied, submit readback after successful presentation.
    pub fn present_frame<T>(
        &mut self,
        present: impl FnOnce(&DrawList, &mut FrameCanvas<'_>, Option<SequenceCapture<'_>>) -> Result<T>,
    ) -> Result<T> {
        self.0.present_frame(present)
    }

    /// Borrow the current draw list and canvas without a screenshot callback.
    pub fn with_frame<T>(
        &mut self,
        render: impl FnOnce(&DrawList, &mut FrameCanvas<'_>) -> Result<T>,
    ) -> Result<T> {
        self.0.with_frame(render)
    }

    /// Hand PCM consumption to a mixer that can run on the host's audio thread.
    /// This can be done once, after startup. The host owns the device lifetime.
    pub fn connect_audio_output(&mut self) -> Result<Mixer> {
        self.0.connect_audio_output()
    }

    /// Fill a stereo buffer with interleaved `f32` PCM in `[-1, 1]`. The sample
    /// rate is in Hz. This call may wait for decoding and is only available
    /// while no separate mixer has been connected.
    pub fn mix_audio_stereo(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        self.0.mix_audio_stereo(sample_rate, output)
    }

    pub fn set_window_content_rect(&mut self, rectangle: Option<Rect>) {
        self.0.set_window_content_rect(rectangle);
    }

    pub fn set_windowed_mode(&mut self, mode: Option<u32>) {
        self.0.set_windowed_mode(mode);
    }

    /// Process window requests through the host. Returns true when the host
    /// should stop the session. Menu rebuild and system properties requests
    /// remain queued for hosts that implement them.
    pub fn process_window_requests(&mut self, window: &mut dyn WindowServices) -> Result<bool> {
        self.0.process_window_requests(window)
    }

    pub fn pending_window_requests(&self) -> &[WindowRequest] {
        self.0.pending_window_requests()
    }

    pub fn take_window_requests(&mut self) -> Vec<WindowRequest> {
        self.0.take_window_requests()
    }

    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.0.take_clipboard_requests()
    }

    /// The current text entry request, identified by the returned Arc. Use
    /// `Arc::ptr_eq` to distinguish successive requests with the same text.
    pub fn text_entry_request(&self) -> Option<Arc<str>> {
        self.0.text_entry_request()
    }

    /// Supply text or cancel with `None`. Returns false for a stale request.
    pub fn complete_text_entry(&mut self, request: &Arc<str>, text: Option<&str>) -> Result<bool> {
        self.0.complete_text_entry(request, text)
    }

    pub fn uses_image_cursor(&self) -> bool {
        self.0.uses_image_cursor()
    }

    pub fn quiz_debug_session(&self, id: u32) -> Option<&QuizDebugSession> {
        self.0.quiz_debug_session(id)
    }

    pub fn visible_quiz_debug_sessions(&self) -> impl Iterator<Item = (u32, &QuizDebugSession)> {
        self.0.visible_quiz_debug_sessions()
    }

    pub fn quiz_debug_input(&mut self, id: u32, input: QuizDebugInput) -> Result<bool> {
        self.0.quiz_debug_input(id, input)
    }

    pub fn set_diagnostic_trace(&mut self, trace: Option<DiagnosticTrace>) {
        self.0.set_diagnostic_trace(trace);
    }

    pub fn take_script_messages(&mut self) -> ScriptMessages {
        self.0.take_script_messages()
    }

    pub fn set_native_trace(&mut self, trace: impl Fn(NativeTraceEvent) + 'static) {
        self.0.set_native_trace(trace);
    }

    pub fn clear_native_trace(&mut self) {
        self.0.clear_native_trace();
    }

    /// Transfer this session to the task API for probes that need to inspect
    /// task objects, VM state, or engine commands directly.
    pub fn into_task_runtime(self) -> TaskRuntime {
        self.0
    }
}
