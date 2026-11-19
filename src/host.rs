//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::session::GameRuntime`] for the session lifecycle.

mod effect;
mod export;
mod platform;
mod resources;

pub use crate::audio::{Mixer, SpeechSynthesizer};
pub use crate::graphics::{
    DrawCommand, DrawList, FrameCanvas, FrameTexture, FrameTriangle, GraphicsBackend,
    ProjectedVertex, Rect, RenderTarget, SoftwareBackend, Surface, TextureAddress, TextureFilter,
    TextureSource,
};
pub use crate::input::{
    ButtonSource, DeviceBinding, DeviceKeys, InputDevice, KeySample, PointerSample,
};
pub use crate::io::{FileServices, ReadSnapshot, ReadSource, SharedSource};
pub use crate::kcs::ScriptSettings;
pub use crate::language::StringTables;
pub use crate::message::{FontRequest, FontServices};
pub use crate::resource::{
    ArchiveConfig, CachedResources, FolderPaths, ResourceCache, ResourceMode,
};
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
pub use platform::{
    AchievementServices, ClipboardRequest, ClipboardText, LocalDateTime, MenuItemRequest,
    PlatformJob, PlatformJobRequest, PlatformJobServices, SystemServices, WindowAction,
    WindowCloseRequest, WindowRequest, WindowServices,
};
pub use resources::ResourceServices;
