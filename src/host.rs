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
