//! Standard game session for embedding CatSystem2. The application supplies
//! [`crate::host`] services and drives the session one frame at a time.

use crate::{Result, host::*, runtime::TaskRuntime};
use std::sync::Arc;

mod error;
mod setup;
pub use error::{AdvanceError, StartupError};
pub use setup::GameSetup;

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

/// Window preferences from the game configuration. The host selects the
/// actual window and display modes independently of the logical canvas size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowPreferences {
    pub window_size: [u32; 2],
    pub fullscreen_size: [u32; 2],
    pub fullscreen: bool,
}

impl WindowPreferences {
    fn from_runtime(runtime: &TaskRuntime) -> Option<Self> {
        let platform = runtime.platform_settings()?;
        let system = runtime.system_settings()?;
        Some(Self {
            window_size: platform.window_size.map(u32::from),
            fullscreen_size: platform.fullscreen_size.map(u32::from),
            fullscreen: system.fullscreen != 0,
        })
    }
}

/// A running game session created by [`GameSetup::start_game`].
pub struct GameRuntime(TaskRuntime);

impl GameRuntime {
    /// Read the window preferences, including changes made by the game.
    pub fn window_preferences(&self) -> Option<WindowPreferences> {
        WindowPreferences::from_runtime(&self.0)
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

    pub fn set_platform_job_services(&mut self, services: impl PlatformJobServices + 'static) {
        self.0.set_platform_job_services(services);
    }

    /// Install the executor for script offscreen drawing and readback. A
    /// requested readback fails if no backend has been installed.
    pub fn set_graphics_backend(&mut self, backend: impl GraphicsBackend + 'static) {
        self.0.set_graphics_backend(backend);
    }

    /// Use a host decoder for future movie loads. Already loaded movies retain
    /// their decoder factory when stopped and played again.
    pub fn set_movie_decoder_factory(&mut self, factory: impl MovieDecoderFactory + 'static) {
        self.0.set_movie_decoder_factory(factory);
    }

    pub fn string_tables(&self) -> &StringTables {
        self.0.string_tables()
    }

    /// Advance one frame, applying pending language changes first. On a
    /// language error, the failed request and remaining requests can be retried.
    pub fn advance(&mut self, time: FrameTime) -> std::result::Result<FrameOutcome, AdvanceError> {
        self.0.advance_frame(time)
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

    /// Inspect registered scripts, including after a failed `advance`.
    /// These positions describe current execution; the error identifies the failure.
    pub fn script_positions(&self) -> impl Iterator<Item = ScriptPosition> + '_ {
        self.0.script_positions()
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
