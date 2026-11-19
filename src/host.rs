//! Interfaces between a game session and its host application.
//!
//! The host provides storage, input and platform services, and handles device
//! presentation. See [`crate::runtime::GameRuntime`] for the session lifecycle.

pub use crate::audio::{Mixer, SpeechSynthesizer};
pub use crate::graphics::{DrawList, GraphicsBackend, RenderTarget};
pub use crate::input::{InputDevice, KeySample, PointerSample};
pub use crate::io::{FileServices, ReadSnapshot, ReadSource, SharedSource};
pub use crate::kcs::{
    AchievementServices, ClipboardRequest, ClipboardText, LocalDateTime, ResourceServices,
    SystemServices, WindowAction, WindowRequest,
};
pub use crate::message::{FontRequest, FontServices};
pub use crate::resource::ResourceMode;
pub use crate::runtime::{
    EffectPlayer, EffectServices, ExportDestination, ExportServices, PlatformJob,
    PlatformJobRequest, PlatformJobServices, ProgramLoader, SequenceCapture, WindowServices,
};
pub use crate::save::SaveServices;
pub use crate::settings::{ConfigurationFile, ConfigurationServices};
