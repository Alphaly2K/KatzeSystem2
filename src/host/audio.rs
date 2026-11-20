use crate::{Result, audio::Pcm};

/// Supplies speech as PCM in the source sample format used by the mixer.
pub trait SpeechSynthesizer {
    /// Receives the two configuration strings as expanded script bytes.
    fn configure(&mut self, _first: &[u8], _second: &[u8]) -> Result<()> {
        Ok(())
    }

    /// Receives expanded text without its terminator. `None` signals a failed utterance.
    fn synthesize(&mut self, text: &[u8]) -> Result<Option<Pcm>>;
}
