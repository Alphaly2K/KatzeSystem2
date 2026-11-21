//! Streaming playback for host-provided movie decoders.

#[cfg(feature = "builtin-mpeg")]
mod ffi;
#[cfg(feature = "builtin-mpeg")]
mod mpeg;
mod playback;

#[cfg(feature = "builtin-mpeg")]
pub use mpeg::{Decoder, MpegDecoderFactory};
pub use playback::{AudioOutput, Player};
