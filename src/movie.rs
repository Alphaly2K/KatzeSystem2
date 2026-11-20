//! Streaming movie playback. The built-in MPEG-1 decoder is optional.

#[cfg(feature = "builtin-mpeg")]
mod ffi;
#[cfg(feature = "builtin-mpeg")]
mod mpeg;
mod playback;

pub use crate::host::{
    MovieAudio as Audio, MovieDecoded as Decoded, MovieFrame as Frame, MovieMetadata as Metadata,
};
#[cfg(feature = "builtin-mpeg")]
pub use mpeg::Decoder;
pub use playback::{AudioOutput, Player};
