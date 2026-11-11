use super::Pcm;
use crate::{Error, Result, read};
use std::ops::Range;

#[derive(Clone, Default)]
pub(super) struct Playback {
    pub playing: bool,
    pub position: usize,
    pub remaining: u32,
    pub completed: u64,
    ended: bool,
    buffered: Buffered,
    stream: Option<Stream>,
}

impl Playback {
    pub fn new(pcm: &Pcm, whole: bool, silence_ms: u32) -> Result<Self> {
        let frame_bytes = u32::from(pcm.channels()) * 2;
        let half_bytes = pcm.sample_rate().wrapping_mul(frame_bytes) / 2;
        let buffer_bytes = half_bytes.wrapping_mul(8);
        let decoder = Decoder {
            // 原入口把无符号乘积交给有符号的六十四位除法。
            silence: (u64::from(pcm.sample_rate()) * u64::from(silence_ms)) as i64 / 1000,
            ..Decoder::default()
        };
        let total_bytes = decoder.length_bytes(pcm);
        let mut playback = Self::default();
        if !whole && total_bytes > buffer_bytes {
            if !half_bytes.is_multiple_of(frame_bytes) {
                return Err(Error::new(0, "流式声音的半秒缓冲块未对齐采样帧"));
            }
            if buffer_bytes == 0
                || half_bytes.checked_mul(8) != Some(buffer_bytes)
                || buffer_bytes as usize > read::MAX_BYTES
            {
                return Err(Error::new(0, "流式声音缓冲区容量无效"));
            }
            let mut stream = Stream {
                samples: vec![0; buffer_bytes as usize / 2],
                half_frames: (half_bytes / frame_bytes) as usize,
                decoder,
                loop_start: 0,
                next_fill: 0,
                previous_cursor: 0,
                accumulated: 0,
                prefetched_loops: 0,
            };
            stream.prefill(pcm)?;
            playback.stream = Some(stream);
        } else {
            playback.buffered.frames = (total_bytes / frame_bytes) as usize;
            playback.buffered.decoder = decoder;
            playback.buffered.prefill(pcm)?;
        }
        Ok(playback)
    }

    pub fn start(&mut self, pcm: &Pcm, count: u32, range: [i64; 2]) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            stream.decoder.end = range[1];
            if self.playing {
                self.position = 0;
                stream.reset(pcm)?;
            }
            stream.loop_start = range[0];
        } else {
            self.buffered.decoder.end = range[1];
            if self.playing {
                self.buffered.reset(pcm)?;
            }
            if self.playing || self.ended {
                self.position = 0;
            }
        }
        self.remaining = count;
        self.completed = 0;
        self.ended = false;
        self.playing = self.stream.is_some() || self.buffered.frames != 0;
        Ok(())
    }

    pub fn change_count(&mut self, count: u32, range: [i64; 2]) {
        if let Some(stream) = &mut self.stream {
            stream.decoder.end = range[1];
            stream.loop_start = range[0];
        } else {
            self.buffered.decoder.end = range[1];
        }
        self.remaining = count;
        self.completed = 0;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn stop(&mut self, pcm: &Pcm) -> Result<()> {
        self.pause();
        self.position = 0;
        self.completed = 0;
        self.ended = false;
        if let Some(stream) = &mut self.stream {
            stream.reset(pcm)?;
        } else {
            self.buffered.reset(pcm)?;
        }
        Ok(())
    }

    pub fn read(&mut self, pcm: &Pcm, output: &mut [i16]) -> Result<()> {
        output.fill(0);
        let channels = pcm.channels() as usize;
        let mut position = 0;
        if let Some(stream) = &mut self.stream {
            while self.playing && position < output.len() {
                let frames = (stream.half_frames - self.position % stream.half_frames)
                    .min((output.len() - position) / channels);
                let count = frames * channels;
                let start = self.position * channels;
                output[position..position + count]
                    .copy_from_slice(&stream.samples[start..start + count]);
                position += count;
                self.position = (self.position + frames) % (stream.half_frames * 8);
                if self.position.is_multiple_of(stream.half_frames)
                    && stream.consume(pcm, self.position, &mut self.remaining)?
                {
                    self.playing = false;
                    self.position = 0;
                }
            }
            return Ok(());
        }
        while self.playing && position < output.len() {
            let frames =
                (self.buffered.frames - self.position).min((output.len() - position) / channels);
            let count = frames * channels;
            self.buffered
                .copy(pcm, self.position, &mut output[position..position + count]);
            position += count;
            self.position += frames;
            if self.position >= self.buffered.frames {
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
        Ok(())
    }
}

#[derive(Clone)]
struct Decoder {
    position: usize,
    ready: bool,
    end: i64,
    silence: i64,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            position: 0,
            ready: true,
            end: -1,
            silence: 0,
        }
    }
}

impl Decoder {
    fn length_bytes(&self, pcm: &Pcm) -> u32 {
        (pcm.samples().len() as u32 * 2)
            .wrapping_add((self.silence * i64::from(pcm.channels()) * 2) as u32)
    }

    fn rewind(&mut self) {
        // 解码重置保留尚未消耗的静音余量。
        self.position = 0;
        self.ready = true;
    }

    fn take(&mut self, pcm: &Pcm, frames: usize) -> Result<(usize, Range<usize>)> {
        let silence = self.silence.max(0).min(frames as i64) as usize;
        self.silence -= silence as i64;
        if silence == frames {
            return Ok((silence, self.position..self.position));
        }
        let frame_bytes = i64::from(pcm.channels()) * 2;
        if self.end != -1
            && (self.end < self.position as i64 || self.end.checked_mul(frame_bytes).is_none())
        {
            return Err(Error::new(0, "声音终点早于预取位置或超出字节范围"));
        }
        let limit = if self.end == -1 {
            pcm.frames()
        } else {
            self.end.min(pcm.frames() as i64) as usize
        };
        let count = (frames - silence).min(limit - self.position);
        let source = self.position..self.position + count;
        self.position += count;
        self.ready &= self.position < limit;
        Ok((silence, source))
    }
}

#[derive(Clone, Default)]
struct Buffered {
    frames: usize,
    silence: usize,
    source_frames: usize,
    decoder: Decoder,
}

impl Buffered {
    fn prefill(&mut self, pcm: &Pcm) -> Result<()> {
        let (silence, source) = self.decoder.take(pcm, self.frames)?;
        self.silence = silence;
        self.source_frames = source.len();
        Ok(())
    }

    fn reset(&mut self, pcm: &Pcm) -> Result<()> {
        self.decoder.rewind();
        self.prefill(pcm)
    }

    fn copy(&self, pcm: &Pcm, position: usize, output: &mut [i16]) {
        // 整段缓冲区由静音、源 PCM 和尾零组成，区间表示无需复制长静音。
        let channels = pcm.channels() as usize;
        let first = position.max(self.silence);
        let last = (position + output.len() / channels).min(self.silence + self.source_frames);
        if first < last {
            output[(first - position) * channels..(last - position) * channels].copy_from_slice(
                &pcm.samples()[(first - self.silence) * channels..(last - self.silence) * channels],
            );
        }
    }
}

#[derive(Clone)]
struct Stream {
    samples: Vec<i16>,
    half_frames: usize,
    decoder: Decoder,
    loop_start: i64,
    next_fill: usize,
    previous_cursor: usize,
    accumulated: i64,
    prefetched_loops: u32,
}

impl Stream {
    fn fill(&mut self, pcm: &Pcm, offset: usize, frames: usize) -> Result<usize> {
        let channels = pcm.channels() as usize;
        let output = &mut self.samples[offset * channels..(offset + frames) * channels];
        output.fill(0);
        let (silence, source) = self.decoder.take(pcm, frames)?;
        output[silence * channels..(silence + source.len()) * channels]
            .copy_from_slice(&pcm.samples()[source.start * channels..source.end * channels]);
        Ok(silence + source.len())
    }

    fn prefill(&mut self, pcm: &Pcm) -> Result<()> {
        self.fill(pcm, 0, self.half_frames * 8)?;
        Ok(())
    }

    fn reset(&mut self, pcm: &Pcm) -> Result<()> {
        self.next_fill = 0;
        self.previous_cursor = 0;
        self.accumulated = 0;
        self.prefetched_loops = 0;
        self.decoder.rewind();
        self.prefill(pcm)
    }

    fn consume(&mut self, pcm: &Pcm, cursor: usize, remaining: &mut u32) -> Result<bool> {
        if cursor / self.half_frames == self.next_fill {
            return Ok(false);
        }
        let frame_bytes = u32::from(pcm.channels()) * 2;
        let offset = self.next_fill * self.half_frames;
        let mut filled = 0;
        let mut read_sum = 0u32;
        while filled < self.half_frames {
            filled += self.fill(pcm, offset + filled, self.half_frames - filled)?;
            read_sum = read_sum.wrapping_add(filled as u32 * frame_bytes);
            if filled == self.half_frames || *remaining == 1 {
                break;
            }
            if *remaining != 0 {
                *remaining -= 1;
            }
            if self.loop_start < 0 || self.loop_start > pcm.frames() as i64 {
                return Err(Error::new(0, "流式声音循环起点超出来源范围"));
            }
            let limit = if self.decoder.end == -1 {
                pcm.frames() as i64
            } else {
                self.decoder.end.min(pcm.frames() as i64)
            };
            if self.loop_start >= limit {
                return Err(Error::new(0, "流式声音循环无法继续预取"));
            }
            self.decoder.position = self.loop_start as usize;
            self.decoder.ready = true;
            self.prefetched_loops = self.prefetched_loops.wrapping_add(1);
        }
        self.next_fill = (self.next_fill + 1) % 8;
        let advanced = if cursor < self.previous_cursor {
            self.half_frames * 8 - self.previous_cursor + cursor
        } else {
            cursor - self.previous_cursor
        };
        self.accumulated = self
            .accumulated
            .wrapping_add((advanced as u32 * frame_bytes) as i64);
        self.previous_cursor = cursor;
        if self.prefetched_loops != 0 {
            // 基线归一式把设备字节差与原始采样帧端点组合，且每次只归一一遍。
            let limit = if self.decoder.end == -1 {
                self.decoder.position as i64 * i64::from(frame_bytes)
            } else {
                self.decoder.end
            };
            if self.accumulated >= limit {
                self.accumulated = self
                    .accumulated
                    .wrapping_add(self.loop_start.wrapping_sub(limit));
                self.prefetched_loops -= 1;
            }
        }
        if read_sum == 0 && self.accumulated >= i64::from(self.decoder.length_bytes(pcm)) {
            self.next_fill = 0;
            self.decoder.rewind();
            self.prefill(pcm)?;
            self.accumulated = 0;
            self.previous_cursor = 0;
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_silence_repeats_with_cached_audio_and_is_consumed_by_explicit_refills() {
        let pcm = Pcm::new(10, 2, vec![1, 2, 3, 4, 5, 6]).unwrap();
        let mut voice = Playback::new(&pcm, false, 200).unwrap();
        voice.start(&pcm, 2, [0, -1]).unwrap();
        let mut output = [0; 20];
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(
            output,
            [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(voice.completed, 2);
        assert!(!voice.playing);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut output[..2]).unwrap();
        assert_eq!(&output[..2], [0, 0]);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut output[..10]).unwrap();
        assert_eq!(&output[..10], [1, 2, 3, 4, 5, 6, 0, 0, 0, 0]);
        voice.stop(&pcm).unwrap();
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut output[..10]).unwrap();
        assert_eq!(&output[..10], [1, 2, 3, 4, 5, 6, 0, 0, 0, 0]);
    }

    #[test]
    fn large_silence_uses_wrapped_lengths_and_signed_conversion_without_materializing_padding() {
        let pcm = Pcm::new(44_100, 1, vec![7]).unwrap();
        let mut voice = Playback::new(&pcm, true, u32::MAX).unwrap();
        assert_eq!(voice.buffered.frames, 429_496_686);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        let mut output = [1; 3];
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(output, [0; 3]);
        let pcm = Pcm::new(u32::MAX, 1, vec![9]).unwrap();
        let mut voice = Playback::new(&pcm, true, u32::MAX).unwrap();
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(output, [9, 0, 0]);
    }

    #[test]
    fn silence_selects_streaming_and_stops_do_not_restore_consumed_padding() {
        let short = Pcm::new(10, 1, (1..=23).collect()).unwrap();
        assert!(Playback::new(&short, false, 1700).unwrap().stream.is_none());
        assert!(Playback::new(&short, false, 1800).unwrap().stream.is_some());
        let pcm = Pcm::new(10, 1, (1..=83).collect()).unwrap();
        let mut voice = Playback::new(&pcm, false, 5000).unwrap();
        voice.stop(&pcm).unwrap();
        voice.start(&pcm, 1, [0, -1]).unwrap();
        let mut output = [0; 12];
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(output, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(output, pcm.samples()[..12]);
    }

    #[test]
    fn buffered_stereo_repeats_the_whole_source_and_empty_pcm_terminates() {
        let pcm = Pcm::new(2, 2, (0..16).collect()).unwrap();
        let mut voice = Playback::new(&pcm, false, 0).unwrap();
        voice.start(&pcm, 2, [3, 5]).unwrap();
        let mut output = [0; 34];
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(&output[..16], pcm.samples());
        assert_eq!(&output[16..32], pcm.samples());
        assert_eq!(&output[32..], [0, 0]);
        assert_eq!(
            (voice.position, voice.remaining, voice.completed),
            (0, 1, 2)
        );
        assert!(!voice.playing);
        voice.start(&pcm, 0, [0, -1]).unwrap();
        voice.read(&pcm, &mut output).unwrap();
        assert!(voice.playing);
        assert_eq!(&output[32..], [0, 1]);
        voice.stop(&pcm).unwrap();
        let empty = Pcm::new(2, 2, vec![]).unwrap();
        voice = Playback::new(&empty, false, 0).unwrap();
        voice.start(&empty, 0, [0, -1]).unwrap();
        voice.read(&empty, &mut output).unwrap();
        assert_eq!(output, [0; 34]);
        assert!(!voice.playing);
    }

    #[test]
    fn streaming_ends_on_buffer_consumption_after_the_partial_final_block() {
        let pcm = Pcm::new(10, 2, (1..=166).collect()).unwrap();
        let mut voice = Playback::new(&pcm, false, 0).unwrap();
        voice.start(&pcm, 1, [0, -1]).unwrap();
        let mut output = [0; 166];
        voice.read(&pcm, &mut output).unwrap();
        assert_eq!(output, pcm.samples());
        assert!(voice.playing);
        assert_eq!(
            (voice.position, voice.remaining, voice.completed),
            (3, 1, 0)
        );
        let mut tail = [1; 4];
        voice.read(&pcm, &mut tail).unwrap();
        assert_eq!(tail, [0; 4]);
        assert!(!voice.playing);
        assert_eq!(voice.position, 0);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut tail).unwrap();
        assert_eq!(tail, [1, 2, 3, 4]);
    }

    #[test]
    fn streaming_count_changes_and_pause_keep_the_prefetched_loop_samples() {
        let pcm = Pcm::new(10, 1, (1..=83).collect()).unwrap();
        let mut voice = Playback::new(&pcm, false, 0).unwrap();
        voice.start(&pcm, 2, [5, 60]).unwrap();
        let mut first = [0; 25];
        voice.read(&pcm, &mut first).unwrap();
        assert_eq!(first, pcm.samples()[..25]);
        assert_eq!(voice.remaining, 1);
        let mut rest = [0; 15];
        voice.read(&pcm, &mut rest).unwrap();
        assert_eq!(rest, pcm.samples()[25..40]);
        voice.pause();
        voice.change_count(1, [0, -1]);
        voice.read(&pcm, &mut rest).unwrap();
        assert_eq!(rest, [0; 15]);
        assert_eq!(voice.position, 0);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        let mut prefetched = [0; 40];
        voice.read(&pcm, &mut prefetched).unwrap();
        assert_eq!(&prefetched[..20], &pcm.samples()[40..60]);
        assert_eq!(&prefetched[20..], &pcm.samples()[5..25]);
        voice.start(&pcm, 2, [0, -1]).unwrap();
        voice.read(&pcm, &mut first).unwrap();
        assert_eq!(first, pcm.samples()[..25]);
        voice.stop(&pcm).unwrap();
        assert_eq!(voice.position, 0);
        voice.start(&pcm, 1, [0, -1]).unwrap();
        voice.read(&pcm, &mut rest).unwrap();
        assert_eq!(rest, pcm.samples()[..15]);
    }

    #[test]
    fn invalid_streaming_blocks_and_nonprogressing_ranges_return_diagnostics() {
        let unaligned = Pcm::new(3, 1, vec![1; 20]).unwrap();
        assert!(Playback::new(&unaligned, false, 0).is_err());
        let pcm = Pcm::new(10, 1, vec![1; 83]).unwrap();
        let mut voice = Playback::new(&pcm, false, 0).unwrap();
        voice.start(&pcm, 0, [0, 1]).unwrap();
        assert!(voice.read(&pcm, &mut [0; 5]).is_err());
        voice.stop(&pcm).unwrap();
        voice.start(&pcm, 0, [83, -1]).unwrap();
        assert!(voice.read(&pcm, &mut [0; 100]).is_err());
    }
}
