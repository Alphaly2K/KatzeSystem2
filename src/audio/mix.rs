use super::SoundState;
use crate::{Error, Result, read};

#[derive(Clone, Default)]
pub(super) struct Resampler {
    rate: u32,
    phase: u64,
    previous: [f32; 2],
    current: [f32; 2],
    primed: bool,
    valid: [bool; 2],
    pub(super) paused: bool,
}

impl Resampler {
    pub(super) fn pending(&self) -> bool {
        !self.paused && self.valid.into_iter().any(|valid| valid)
    }
}

impl SoundState {
    /// 覆写交错立体声浮点采样，应用通道音量和声像并推进播放位置。
    /// 采样率转换使用连续线性插值；调用方按设备消费速度请求采样。
    pub fn mix_stereo(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        self.mix(sample_rate, output, true)
    }

    pub(super) fn mix(&mut self, sample_rate: u32, output: &mut [f32], wait: bool) -> Result<()> {
        if self.output.is_some() {
            return Err(Error::new(0, "声音采样已交给独立混音器"));
        }
        if sample_rate == 0 || !output.len().is_multiple_of(2) || output.len() > read::MAX_BYTES / 4
        {
            return Err(Error::new(0, "声音混音输出格式或容量无效"));
        }
        output.fill(0.0);
        for sound in &mut self.channels {
            let Some(pcm) = &sound.source else { continue };
            if sound.resampler.paused || (!sound.playback.playing && !sound.resampler.pending()) {
                continue;
            }
            let channels = pcm.channels() as usize;
            let gain = sound.pcm_volume.clamp(0, 100) as f32 / 100.0;
            let pan = sound.pcm_pan.clamp(-100, 100) as f32 / 100.0;
            let gains = [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))];
            if pcm.sample_rate() == sample_rate {
                sound.resampler = Resampler::default();
                let mut source = [0; 1024];
                for chunk in output.chunks_mut(1024) {
                    let source = &mut source[..chunk.len() / 2 * channels];
                    let consumed = sound.playback.read(pcm, source, wait)?;
                    for (frame, samples) in
                        chunk.chunks_exact_mut(2).zip(source.chunks_exact(channels))
                    {
                        frame[0] += f32::from(samples[0]) / 32768.0 * gains[0];
                        frame[1] += f32::from(samples[channels - 1]) / 32768.0 * gains[1];
                    }
                    if consumed < source.len() / channels {
                        break;
                    }
                }
            } else {
                let state = &mut sound.resampler;
                if state.rate != sample_rate {
                    *state = Resampler {
                        rate: sample_rate,
                        ..Resampler::default()
                    };
                }
                let mut source = [0; 2];
                'frames: for frame in output.chunks_exact_mut(2) {
                    if !state.primed {
                        let valid = sound.playback.playing;
                        if sound.playback.read(pcm, &mut source[..channels], wait)? == 0 && valid {
                            break 'frames;
                        }
                        state.current = [f32::from(source[0]), f32::from(source[channels - 1])];
                        state.previous = state.current;
                        state.primed = true;
                        state.valid = [valid; 2];
                    }
                    while state.phase >= u64::from(sample_rate) {
                        let valid = sound.playback.playing;
                        if sound.playback.read(pcm, &mut source[..channels], wait)? == 0 && valid {
                            break 'frames;
                        }
                        state.previous = state.current;
                        state.current = [f32::from(source[0]), f32::from(source[channels - 1])];
                        state.phase -= u64::from(sample_rate);
                        state.valid = [state.valid[1], valid];
                    }
                    let fraction = state.phase as f32 / sample_rate as f32;
                    for side in 0..2 {
                        let sample = state.previous[side]
                            + (state.current[side] - state.previous[side]) * fraction;
                        frame[side] += sample / 32768.0 * gains[side];
                    }
                    state.phase += u64::from(pcm.sample_rate());
                }
            }
        }
        if let Some(movie) = &self.movie {
            movie.mix(sample_rate, output, wait)?;
        }
        for sample in output {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{Pcm, playback::Playback};

    fn state(rate: u32, channels: u8, samples: Vec<i16>) -> SoundState {
        let mut state = SoundState::default();
        let pcm: crate::audio::Source = Pcm::new(rate, channels, samples).unwrap().into();
        let mut playback = Playback::new(&pcm, true, 0).unwrap();
        playback.start(&pcm, 1, [0, -1]).unwrap();
        state.channels[0].source = Some(pcm);
        state.channels[0].playback = playback;
        state
    }

    #[test]
    fn mixing_applies_stereo_volume_pan_clipping_and_finishes_playback() {
        let mut sound = state(8000, 2, vec![16384, -16384, 8192, -8192]);
        sound.channels[0].pcm_volume = 50;
        sound.channels[0].pcm_pan = -100;
        let mut output = [1.0; 6];
        sound.mix_stereo(8000, &mut output).unwrap();
        assert_eq!(output, [0.25, 0.0, 0.125, 0.0, 0.0, 0.0]);
        assert!(!sound.channels[0].is_playing());
        let mut sound = state(8000, 1, vec![24576; 3]);
        let mut second = state(8000, 1, vec![24576; 3]);
        std::mem::swap(&mut sound.channels[1], &mut second.channels[0]);
        sound.mix_stereo(8000, &mut output).unwrap();
        assert_eq!(output, [1.0; 6]);
        assert!(sound.mix_stereo(0, &mut output).is_err());
        assert!(sound.mix_stereo(8000, &mut [0.0; 3]).is_err());
    }

    #[test]
    fn resampling_is_continuous_across_chunks_pause_and_end_of_stream() {
        for (source, destination) in [(4, 8), (8, 3)] {
            let samples = vec![0, 8192, 16384, 24576, 16384, 8192];
            let mut whole = state(source, 1, samples.clone());
            let mut split = state(source, 1, samples);
            let mut expected = [0.0; 74];
            whole.mix_stereo(destination, &mut expected).unwrap();
            let mut actual = [0.0; 74];
            let mut cursor = 0;
            for frames in [1, 2, 7, 27] {
                split
                    .mix_stereo(destination, &mut actual[cursor..cursor + frames * 2])
                    .unwrap();
                cursor += frames * 2;
                if cursor == 2 {
                    split.channels[0].playback.pause();
                    split.channels[0].resampler.paused = true;
                    let mut silence = [1.0; 10];
                    split.mix_stereo(destination, &mut silence).unwrap();
                    assert_eq!(silence, [0.0; 10]);
                    let channel = &mut split.channels[0];
                    channel
                        .playback
                        .start(channel.source.as_ref().unwrap(), 1, [0, -1])
                        .unwrap();
                    channel.resampler.paused = false;
                }
            }
            assert_eq!(actual, expected);
            assert!(!whole.channels[0].is_playing());
            assert!(!split.channels[0].is_playing());
            assert!(actual.iter().any(|&sample| sample > 0.0));
        }
    }
}
