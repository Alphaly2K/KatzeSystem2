//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::session::GameRuntime`] for the session lifecycle.

mod audio;
mod diagnostics;
mod effect;
mod export;
mod font;
mod graphics;
mod input;
mod platform;
mod resources;
mod storage;

pub use crate::audio::{Mixer, Pcm};
pub use crate::graphics::{
    DrawCommand, DrawList, FrameCanvas, FrameTexture, FrameTriangle, ProjectedVertex, Rect,
    RenderTarget, SoftwareBackend, Surface, TextureAddress, TextureFilter, TextureSource,
};
pub use crate::input::DeviceKeys;
pub use crate::kcs::ScriptSettings;
pub use crate::language::StringTables;
pub use crate::message::{GlyphMask, TextEncoding};
pub use crate::resource::{
    ArchiveConfig, CachedResources, FolderPaths, ResourceCache, ResourceMode,
};
pub use crate::runtime::{QuizDebugInput, QuizDebugSession, SequenceCapture};
pub use audio::SpeechSynthesizer;
pub(crate) use diagnostics::trace_detail;
pub use diagnostics::{
    DiagnosticTrace, NativeInvocation, NativeTraceEvent, ScriptLocation, ScriptMessages,
    ScriptPosition, TraceCategory,
};
pub use effect::{EffectPlayer, EffectServices};
pub use export::{ExportDestination, ExportServices};
pub use font::{FontRequest, FontServices};
pub use graphics::GraphicsBackend;
pub use input::{ButtonSource, DeviceBinding, InputDevice, KeySample, PointerSample};
pub use platform::{
    AchievementServices, ClipboardRequest, ClipboardText, LocalDateTime, MenuItemRequest,
    PlatformJob, PlatformJobRequest, PlatformJobServices, SystemServices, WindowAction,
    WindowCloseRequest, WindowRequest, WindowServices,
};
pub use resources::ResourceServices;
pub use storage::{
    ConfigurationFile, ConfigurationServices, FileServices, ReadSnapshot, ReadSource, SaveServices,
    SharedSource,
};
