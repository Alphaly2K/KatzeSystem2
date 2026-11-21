//! Streaming playback for host-provided movie decoders.

#[cfg(feature = "builtin-mpeg")]
mod ffi;
#[cfg(feature = "builtin-mpeg")]
mod mpeg;
mod playback;

pub use crate::host::movie::{
    MovieAudio as Audio, MovieDecoded as Decoded, MovieFrame as Frame, MovieMetadata as Metadata,
};
#[cfg(feature = "builtin-mpeg")]
pub use mpeg::{Decoder, MpegDecoderFactory};
pub use playback::{AudioOutput, Player};
