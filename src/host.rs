//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::runtime::GameRuntime`] for the session lifecycle.

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
    SystemServices, WindowAction, WindowRequest,
};
pub use crate::message::{FontRequest, FontServices};
pub use crate::resource::ResourceMode;
pub use crate::runtime::{
    DiagnosticTrace, ProgramLoader, ScriptMessages, SequenceCapture, TraceCategory,
};
pub use crate::save::SaveServices;
pub use crate::settings::{ConfigurationFile, ConfigurationServices};
pub use effect::{EffectPlayer, EffectServices};
pub use export::{ExportDestination, ExportServices};
pub use platform::{PlatformJob, PlatformJobRequest, PlatformJobServices, WindowServices};
