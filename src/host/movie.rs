//! Movie decoders for the formats supported by the host.

use super::{graphics::Surface, storage::SharedSource};
use crate::Result;

/// Top-down BGRA frame. The presentation time is in seconds from stream start.
pub struct MovieFrame {
    pub time: f64,
    pub surface: Surface,
}

/// Interleaved stereo PCM starting at `time` seconds from stream start.
pub struct MovieAudio {
    pub time: f64,
    pub samples: Vec<f32>,
}

/// The last newly decoded frame, new audio, and the end-of-stream flag.
pub struct MovieDecoded {
    pub video: Option<MovieFrame>,
    pub audio: Option<MovieAudio>,
    pub ended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovieMetadata {
    pub size: [u32; 2],
    pub frame_rate: f64,
    /// Sample rate in Hz. Zero means the movie has no audio track.
    pub sample_rate: u32,
    /// Duration in seconds.
    pub duration: f64,
}

/// One stream's decode state, moved to the runtime's playback worker.
pub trait MovieDecoder: Send {
    fn metadata(&self) -> MovieMetadata;
    /// Seconds decoded since start. Must increase on each step before EOF.
    fn time(&self) -> f64;
    /// Return to the start of the same stream for a playback loop.
    fn rewind(&mut self) -> Result<()>;
    /// Decode up to the next playback step. Return even-length, stereo `f32`
    /// samples in `[-1, 1]` at the declared sample rate.
    fn advance(&mut self, seconds: f64) -> Result<MovieDecoded>;
}

/// Open a game resource at stream start. The runtime can call again when a
/// stopped movie is restarted; metadata must stay the same for that resource.
pub trait MovieDecoderFactory: Send + Sync {
    fn open(&self, source: SharedSource) -> Result<Box<dyn MovieDecoder>>;
}
