//! PCM output and optional speech synthesis.

pub use crate::audio::output::Mixer;
use crate::{Error, Result, read};

/// Interleaved signed 16-bit PCM. Each frame holds one sample per channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    sample_rate: u32,
    channels: u8,
    samples: std::sync::Arc<[i16]>,
}

impl Pcm {
    /// Create mono or stereo PCM at a nonzero sample rate.
    pub fn new(sample_rate: u32, channels: u8, samples: Vec<i16>) -> Result<Self> {
        if sample_rate == 0 || !matches!(channels, 1 | 2) {
            return Err(Error::new(0, "PCM 需要非零采样率及单声道或双声道"));
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(Error::new(0, "PCM 数据未对齐完整采样帧"));
        }
        if samples.len() > read::MAX_BYTES / 2 {
            return Err(Error::new(0, "PCM 数据超过读取上限"));
        }
        Ok(Self {
            sample_rate,
            channels,
            samples: samples.into(),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u8 {
        self.channels
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
}

/// Supplies speech as PCM in the source sample format used by the mixer.
pub trait SpeechSynthesizer {
    /// Receives the two configuration strings as expanded script bytes.
    fn configure(&mut self, _first: &[u8], _second: &[u8]) -> Result<()> {
        Ok(())
    }

    /// Receives expanded text without its terminator. `None` signals a failed utterance.
    fn synthesize(&mut self, text: &[u8]) -> Result<Option<Pcm>>;
}
