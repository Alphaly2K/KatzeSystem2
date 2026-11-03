use super::Pcm;
use crate::{Error, Result};

#[derive(Default)]
pub(super) struct Playback {
    pub playing: bool,
    pub position: usize,
    pub remaining: u32,
    pub completed: u64,
    ended: bool,
}

impl Playback {
    pub fn start(&mut self, pcm: &Pcm, count: u32) -> Result<()> {
        ensure_buffered(pcm)?;
        if self.playing || self.ended {
            self.position = 0;
        }
        self.remaining = count;
        self.completed = 0;
        self.ended = false;
        self.playing = pcm.frames() != 0;
        Ok(())
    }

    pub fn change_count(&mut self, pcm: &Pcm, count: u32) -> Result<()> {
        ensure_buffered(pcm)?;
        self.remaining = count;
        self.completed = 0;
        Ok(())
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn stop(&mut self) {
        self.pause();
        self.position = 0;
        self.completed = 0;
        self.ended = false;
    }

    pub fn read(&mut self, pcm: &Pcm, output: &mut [i16]) {
        output.fill(0);
        let channels = pcm.channels() as usize;
        let mut position = 0;
        while self.playing && position < output.len() {
            if self.position < pcm.frames() {
                let frames =
                    (pcm.frames() - self.position).min((output.len() - position) / channels);
                let count = frames * channels;
                let start = self.position * channels;
                output[position..position + count]
                    .copy_from_slice(&pcm.samples()[start..start + count]);
                position += count;
                self.position += frames;
            }
            if self.position >= pcm.frames() {
                self.completed = self.completed.saturating_add(1);
                if self.remaining == 1 {
                    self.playing = false;
                    self.ended = true;
                    self.position = 0;
                } else {
                    if self.remaining != 0 {
                        self.remaining -= 1;
                    }
                    self.position = 0;
                }
            }
        }
    }
}

fn ensure_buffered(pcm: &Pcm) -> Result<()> {
    let frame_bytes = u32::from(pcm.channels()) * 2;
    let buffer_bytes = (pcm.sample_rate().wrapping_mul(frame_bytes) / 2).wrapping_mul(8);
    if pcm.samples().len() * 2 > buffer_bytes as usize {
        return Err(Error::new(0, "流式声音播放尚未接入预取缓冲区"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_stereo_repeats_the_whole_source_and_empty_pcm_terminates() {
        let pcm = Pcm::new(2, 2, (0..16).collect()).unwrap();
        let mut voice = Playback::default();
        voice.start(&pcm, 2).unwrap();
        let mut output = [0; 34];
        voice.read(&pcm, &mut output);
        assert_eq!(&output[..16], pcm.samples());
        assert_eq!(&output[16..32], pcm.samples());
        assert_eq!(&output[32..], [0, 0]);
        assert_eq!(
            (voice.position, voice.remaining, voice.completed),
            (0, 1, 2)
        );
        assert!(!voice.playing);
        voice.start(&pcm, 0).unwrap();
        voice.read(&pcm, &mut output);
        assert!(voice.playing);
        assert_eq!(&output[32..], [0, 1]);
        voice.stop();
        let empty = Pcm::new(2, 2, vec![]).unwrap();
        voice.start(&empty, 0).unwrap();
        voice.read(&empty, &mut output);
        assert_eq!(output, [0; 34]);
        assert!(!voice.playing);
        let streamed = Pcm::new(2, 2, vec![1; 18]).unwrap();
        assert!(voice.start(&streamed, 1).is_err());
        assert!(!voice.playing);
    }
}
