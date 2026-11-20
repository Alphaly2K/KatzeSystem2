//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::session::GameRuntime`] for the session lifecycle.
//! Import each service and its data from the module for that capability.

pub mod audio;
pub mod diagnostics;
pub mod effect;
pub mod font;
pub mod graphics;
pub mod input;
pub mod media;
pub mod model;
pub mod movie;
pub mod platform;
pub mod resources;
pub mod storage;

// Keep existing imports working while documenting the capability modules.
#[doc(hidden)]
pub use crate::graphics::{RenderTarget, SoftwareBackend};
#[doc(hidden)]
pub use crate::language::StringTables;
#[doc(hidden)]
pub use audio::{Mixer, Pcm, SpeechSynthesizer};
pub(crate) use diagnostics::trace_detail;
#[doc(hidden)]
pub use diagnostics::{
    DiagnosticTrace, NativeInvocation, NativeTraceEvent, QuizDebugInput, QuizDebugSession,
    ScriptLocation, ScriptMessages, ScriptPosition, TraceCategory,
};
#[doc(hidden)]
pub use effect::{EffectPlayer, EffectServices};
#[doc(hidden)]
pub use font::{FontRequest, FontServices, GlyphMask, TextEncoding};
#[doc(hidden)]
pub use graphics::{
    DrawCommand, DrawList, FrameCanvas, FrameTexture, FrameTriangle, GraphicsBackend,
    ProjectedVertex, Rect, SequenceCapture, Surface, TextureAddress, TextureFilter, TextureSource,
};
#[doc(hidden)]
pub use input::{ButtonSource, DeviceBinding, DeviceKeys, InputDevice, KeySample, PointerSample};
#[doc(hidden)]
pub use movie::{
    MovieAudio, MovieDecoded, MovieDecoder, MovieDecoderFactory, MovieFrame, MovieMetadata,
};
#[doc(hidden)]
pub use platform::{
    AchievementServices, ClipboardRequest, ClipboardText, LocalDateTime, MenuItemRequest,
    PlatformJob, PlatformJobRequest, PlatformJobServices, SystemServices, WindowAction,
    WindowCloseRequest, WindowRequest, WindowServices,
};
#[doc(hidden)]
pub use resources::{
    ArchiveConfig, CachedResources, ResourceCache, ResourceMode, ResourceServices,
};
#[doc(hidden)]
pub use storage::{
    ConfigurationFile, ConfigurationServices, ExportDestination, ExportServices, FileServices,
    FolderPaths, ReadSnapshot, ReadSource, SaveServices, ScriptSettings, SharedSource,
};
