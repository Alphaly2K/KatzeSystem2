//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::session::GameRuntime`] for the session lifecycle.

mod effect;
mod export;
mod platform;

pub use crate::audio::{Mixer, SpeechSynthesizer};
pub use crate::graphics::{
    DrawCommand, DrawList, FrameTexture, GraphicsBackend, ProjectedVertex, Rect, RenderTarget,
    SoftwareBackend, Surface, TextureAddress, TextureFilter, TextureSource,
};
pub use crate::input::{
    ButtonSource, DeviceBinding, DeviceKeys, InputDevice, KeySample, PointerSample,
};
pub use crate::io::{FileServices, ReadSnapshot, ReadSource, SharedSource};
pub use crate::kcs::{
    AchievementServices, ClipboardRequest, ClipboardText, LocalDateTime, ResourceServices,
    ScriptSettings, SystemServices, WindowAction, WindowRequest,
};
pub use crate::language::StringTables;
pub use crate::message::{FontRequest, FontServices};
pub use crate::resource::{ArchiveConfig, FolderPaths, ResourceMode};
pub use crate::runtime::{
    DiagnosticTrace, NativeInvocation, NativeTraceEvent, ProgramLoader, QuizDebugInput,
    QuizDebugSession, ScriptMessages, SequenceCapture, TraceCategory,
};
pub use crate::save::SaveServices;
pub use crate::settings::{
    ConfigurationFile, ConfigurationServices, PlatformSettings, SystemSettings,
};
pub use effect::{EffectPlayer, EffectServices};
pub use export::{ExportDestination, ExportServices};
pub use platform::{PlatformJob, PlatformJobRequest, PlatformJobServices, WindowServices};
