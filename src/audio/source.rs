use super::{Pcm, VorbisDecoder};
use crate::io::SharedSource;
use crate::{Error, Result};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};

/// 可重复读取的音频来源；流式 Vorbis 先准备四秒采样，再由独立线程继续解码。
#[derive(Clone)]
pub struct Source {
    rate: u32,
    channels: u8,
    frames: usize,
    data: Data,
}

#[derive(Clone)]
enum Data {
    Pcm(Pcm),
    Streaming(Arc<Shared>),
}

struct Shared {
    prefix: Arc<[i16]>,
    buffer: Mutex<Buffer>,
    changed: Condvar,
    complete: OnceLock<Result<Pcm>>,
}

struct Buffer {
    chunks: Vec<(usize, Arc<[i16]>)>,
    ready: usize,
}

impl From<Pcm> for Source {
    fn from(pcm: Pcm) -> Self {
        Self {
            rate: pcm.sample_rate(),
            channels: pcm.channels(),
            frames: pcm.frames(),
            data: Data::Pcm(pcm),
        }
    }
}

impl Source {
    pub fn from_vorbis(source: SharedSource, whole: bool) -> Result<Self> {
        let mut decoder = VorbisDecoder::from_shared(source)?;
        let rate = decoder.sample_rate();
        let channels = decoder.channels();
        let frames = decoder.frames();
        let mut samples = Vec::new();
        let prefill_frames = (u64::from(rate) * 4).min(frames as u64) as usize;
        let prefill = prefill_frames * channels as usize;
        if whole || frames <= prefill_frames {
            while let Some(packet) = decoder.read_packet()? {
                samples.extend(packet);
            }
            return Pcm::new(rate, channels, samples).map(Self::from);
        }
        while samples.len() < prefill {
            let Some(packet) = decoder.read_packet()? else {
                break;
            };
            samples.extend(packet);
        }
        Self::prefetched(rate, channels, frames, samples, move || {
            decoder.read_packet()
        })
    }

    fn prefetched(
        rate: u32,
        channels: u8,
        frames: usize,
        samples: Vec<i16>,
        next: impl FnMut() -> Result<Option<Vec<i16>>> + Send + 'static,
    ) -> Result<Self> {
        let prefix: Arc<[i16]> = samples.as_slice().into();
        let shared = Arc::new(Shared {
            prefix: prefix.clone(),
            buffer: Mutex::new(Buffer {
                ready: samples.len(),
                chunks: vec![(0, prefix)],
            }),
            changed: Condvar::new(),
            complete: OnceLock::new(),
        });
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("katze-audio-decode".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prefetch(&weak, rate, channels, frames, samples, next)
                }))
                .unwrap_or_else(|_| Err(Error::new(0, "声音预取线程异常退出")));
                if let Some(shared) = weak.upgrade() {
                    if let Some(result) = result.transpose() {
                        let _ = shared.complete.set(result);
                        let chunks = {
                            let mut buffer =
                                shared.buffer.lock().unwrap_or_else(|e| e.into_inner());
                            let chunks = std::mem::take(&mut buffer.chunks);
                            shared.changed.notify_all();
                            chunks
                        };
                        drop(chunks);
                    }
                }
            })
            .map_err(|error| Error::new(0, format!("声音预取线程创建失败：{error}")))?;
        Ok(Self {
            rate,
            channels,
            frames,
            data: Data::Streaming(shared),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u8 {
        self.channels
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// 等待剩余解码完成并借用完整 PCM；适合离线处理和显式数据查询。
    pub fn pcm(&self) -> Result<&Pcm> {
        match &self.data {
            Data::Pcm(pcm) => Ok(pcm),
            Data::Streaming(shared) => {
                let mut buffer = shared.buffer.lock().unwrap_or_else(|e| e.into_inner());
                while shared.complete.get().is_none() {
                    buffer = shared
                        .changed
                        .wait(buffer)
                        .unwrap_or_else(|e| e.into_inner());
                }
                shared
                    .complete
                    .get()
                    .unwrap()
                    .as_ref()
                    .map_err(Clone::clone)
            }
        }
    }

    /// 读取完整采样帧；所需数据仍在预取时等待，失败保留输出。
    pub fn read_frames(&self, offset: usize, output: &mut [i16]) -> Result<()> {
        self.read(offset, output, true).map(|_| ())
    }

    /// 数据或缓冲锁暂不可用时返回 false 并保留输出，不等待解码线程。
    pub fn try_read_frames(&self, offset: usize, output: &mut [i16]) -> Result<bool> {
        self.read(offset, output, false)
    }

    fn read(&self, offset: usize, output: &mut [i16], wait: bool) -> Result<bool> {
        let channels = self.channels as usize;
        if !output.len().is_multiple_of(channels)
            || offset
                .checked_add(output.len() / channels)
                .is_none_or(|end| end > self.frames)
        {
            return Err(Error::new(0, "声音来源读取范围或采样帧对齐无效"));
        }
        if output.is_empty() {
            return Ok(true);
        }
        let start = offset * channels;
        let end = start + output.len();
        let copy = |pcm: &Pcm, output: &mut [i16]| {
            output.copy_from_slice(&pcm.samples()[start..end]);
            Ok(true)
        };
        let Data::Streaming(shared) = &self.data else {
            let Data::Pcm(pcm) = &self.data else {
                unreachable!()
            };
            return copy(pcm, output);
        };
        if end <= shared.prefix.len() {
            output.copy_from_slice(&shared.prefix[start..end]);
            return Ok(true);
        }
        loop {
            if let Some(result) = shared.complete.get() {
                return copy(result.as_ref().map_err(Clone::clone)?, output);
            }
            let mut buffer = if wait {
                shared.buffer.lock().unwrap_or_else(|e| e.into_inner())
            } else {
                match shared.buffer.try_lock() {
                    Ok(buffer) => buffer,
                    Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
                }
            };
            if let Some(result) = shared.complete.get() {
                return copy(result.as_ref().map_err(Clone::clone)?, output);
            }
            if end <= buffer.ready {
                let first = buffer
                    .chunks
                    .partition_point(|(at, _)| *at <= start)
                    .saturating_sub(1);
                let mut at = start;
                for (base, chunk) in &buffer.chunks[first..] {
                    let from = at - base;
                    let count = (chunk.len() - from).min(end - at);
                    output[at - start..at - start + count]
                        .copy_from_slice(&chunk[from..from + count]);
                    at += count;
                    if at == end {
                        break;
                    }
                }
                return Ok(true);
            }
            if !wait {
                return Ok(false);
            }
            buffer = shared
                .changed
                .wait(buffer)
                .unwrap_or_else(|e| e.into_inner());
            drop(buffer);
        }
    }
}

fn prefetch(
    shared: &Weak<Shared>,
    rate: u32,
    channels: u8,
    frames: usize,
    mut samples: Vec<i16>,
    mut next: impl FnMut() -> Result<Option<Vec<i16>>>,
) -> Result<Option<Pcm>> {
    while shared.strong_count() != 0 {
        let Some(packet) = next()? else {
            if samples.len() != frames * channels as usize {
                return Err(Error::new(0, "声音预取采样数量与声明长度不符"));
            }
            return Pcm::new(rate, channels, samples).map(Some);
        };
        if !packet.len().is_multiple_of(channels as usize)
            || samples
                .len()
                .checked_add(packet.len())
                .is_none_or(|end| end > frames * channels as usize)
        {
            return Err(Error::new(0, "声音预取采样数量或帧对齐无效"));
        }
        let Some(shared) = shared.upgrade() else {
            return Ok(None);
        };
        let mut buffer = shared.buffer.lock().unwrap_or_else(|e| e.into_inner());
        let at = buffer.ready;
        buffer.chunks.push((at, packet.as_slice().into()));
        buffer.ready += packet.len();
        shared.changed.notify_all();
        drop(buffer);
        samples.extend(packet);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn device_starvation_preserves_samples_resampling_and_other_voices() {
        use crate::audio::{SoundState, playback::Playback};
        for rate in [10, 7, 16] {
            let samples: Vec<i16> = (8192..8275).collect();
            let (sender, receiver) = mpsc::channel();
            let source = Source::prefetched(10, 1, 83, samples[..40].to_vec(), move || {
                receiver.recv().unwrap()
            })
            .unwrap();
            let build = |source: Source| {
                let mut state = SoundState::default();
                let sound = &mut state.channels[0];
                sound.playback = Playback::new(&source, false, 0).unwrap();
                sound.playback.start(&source, 2, [5, 60]).unwrap();
                sound.source = Some(source);
                sound.pcm_pan = 100;
                sound.requested_volume = 100.0;
                sound.loop_range = [5, 60];
                state
            };
            let mut expected = build(Pcm::new(10, 1, samples.clone()).unwrap().into());
            let mut actual = build(source.clone());
            let other: Source = Pcm::new(rate, 1, vec![1024; 512]).unwrap().into();
            actual.channels[1].playback = Playback::new(&other, true, 0).unwrap();
            actual.channels[1]
                .playback
                .start(&other, 1, [0, -1])
                .unwrap();
            actual.channels[1].source = Some(other);
            actual.channels[1].pcm_pan = -100;
            let mut mixer = actual.connect_output().unwrap();
            let mut first = [0.0; 64];
            mixer.render(rate, &mut first).unwrap();
            assert!(first.chunks_exact(2).all(|frame| frame[0] == 0.03125));
            let count = first
                .chunks_exact(2)
                .take_while(|frame| frame[1] != 0.0)
                .count();
            assert!(count > 0 && count < first.len() / 2);
            assert!(
                first[count * 2..]
                    .chunks_exact(2)
                    .all(|frame| frame[1] == 0.0)
            );
            let position = actual.channel(0).unwrap().position_frames();
            actual.control_pair(0, 184, 0).unwrap();
            mixer.render(rate, &mut [0.0; 4]).unwrap();
            actual.start(0, 2, 0).unwrap();
            mixer.render(rate, &mut [0.0; 4]).unwrap();
            assert_eq!(actual.channel(0).unwrap().position_frames(), position);
            sender.send(Ok(Some(samples[40..].to_vec()))).unwrap();
            sender.send(Ok(None)).unwrap();
            source.pcm().unwrap();
            let mut rest = [0.0; 300];
            mixer.render(rate, &mut rest).unwrap();
            let mut reference = vec![0.0; count * 2 + rest.len()];
            expected.mix_stereo(rate, &mut reference).unwrap();
            let actual_right: Vec<_> = first[..count * 2]
                .chunks_exact(2)
                .chain(rest.chunks_exact(2))
                .map(|frame| frame[1])
                .collect();
            let expected_right: Vec<_> = reference.chunks_exact(2).map(|frame| frame[1]).collect();
            assert_eq!(actual_right, expected_right, "output rate {rate}");
        }
    }

    #[test]
    fn streamed_vorbis_preserves_prefix_tail_and_complete_pcm() {
        let bytes = include_bytes!("../../tests/fixtures/audio/mono-8000-long.ogg");
        let expected = super::super::decode_vorbis(bytes).unwrap();
        let source = Source::from_vorbis(Arc::new(bytes.to_vec()), false).unwrap();
        assert!(matches!(source.data, Data::Streaming(_)));
        assert_eq!(source.sample_rate(), expected.sample_rate());
        assert_eq!(source.channels(), expected.channels());
        assert_eq!(source.frames(), expected.frames());
        let mut prefix = [0; 256];
        source.read_frames(0, &mut prefix).unwrap();
        assert_eq!(prefix, expected.samples()[..prefix.len()]);
        let mut tail = [0; 257];
        source
            .read_frames(expected.frames() - tail.len(), &mut tail)
            .unwrap();
        assert_eq!(tail, expected.samples()[expected.frames() - tail.len()..]);
        assert_eq!(source.pcm().unwrap(), &expected);
        let clone = source.clone();
        drop(source);
        assert_eq!(clone.pcm().unwrap(), &expected);
        let mut invalid = [7; 2];
        assert!(clone.try_read_frames(clone.frames(), &mut invalid).is_err());
        assert_eq!(invalid, [7; 2]);
    }

    #[test]
    fn pending_reads_preserve_output_and_wake_with_data_or_errors() {
        for fail in [false, true] {
            let (sender, receiver) = mpsc::channel::<Result<Option<Vec<i16>>>>();
            let source =
                Source::prefetched(1, 2, 4, vec![1, 2, 3, 4], move || receiver.recv().unwrap())
                    .unwrap();
            let mut output = [77; 4];
            assert!(source.try_read_frames(0, &mut output).unwrap());
            assert_eq!(output, [1, 2, 3, 4]);
            output.fill(77);
            assert!(!source.try_read_frames(2, &mut output).unwrap());
            assert_eq!(output, [77; 4]);
            let Data::Streaming(shared) = &source.data else {
                unreachable!()
            };
            let guard = shared.buffer.lock().unwrap();
            assert!(source.try_read_frames(0, &mut output).unwrap());
            output.fill(77);
            assert!(!source.try_read_frames(2, &mut output).unwrap());
            assert_eq!(output, [77; 4]);
            assert!(source.try_read_frames(4, &mut []).unwrap());
            drop(guard);
            let waiting = source.clone();
            let worker = std::thread::spawn(move || {
                let mut output = [77; 4];
                (waiting.read_frames(2, &mut output), output)
            });
            if fail {
                sender
                    .send(Err(Error::new(19, "synthetic decode failure")))
                    .unwrap();
                assert_eq!(
                    worker.join().unwrap(),
                    (Err(Error::new(19, "synthetic decode failure")), [77; 4])
                );
                assert_eq!(source.pcm().unwrap_err().offset, 19);
            } else {
                sender.send(Ok(Some(vec![5, 6, 7, 8]))).unwrap();
                sender.send(Ok(None)).unwrap();
                assert_eq!(worker.join().unwrap(), (Ok(()), [5, 6, 7, 8]));
                assert_eq!(source.pcm().unwrap().samples(), [1, 2, 3, 4, 5, 6, 7, 8]);
            }
        }
    }

    #[test]
    fn releasing_the_source_cancels_prefetch_after_the_current_packet() {
        struct Producer {
            entered: mpsc::Sender<()>,
            proceed: mpsc::Receiver<()>,
            dropped: mpsc::Sender<()>,
        }
        impl Producer {
            fn packet(&self) -> Result<Option<Vec<i16>>> {
                self.entered.send(()).unwrap();
                self.proceed.recv().unwrap();
                Ok(Some(vec![3, 4]))
            }
        }
        impl Drop for Producer {
            fn drop(&mut self) {
                let _ = self.dropped.send(());
            }
        }
        let (entered, enter_receiver) = mpsc::channel();
        let (proceed, proceed_receiver) = mpsc::channel();
        let (dropped, drop_receiver) = mpsc::channel();
        let producer = Producer {
            entered,
            proceed: proceed_receiver,
            dropped,
        };
        let source = Source::prefetched(1, 1, 4, vec![1, 2], move || producer.packet()).unwrap();
        enter_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let Data::Streaming(shared) = &source.data else {
            unreachable!()
        };
        let weak = Arc::downgrade(shared);
        drop(source);
        assert!(weak.upgrade().is_none());
        proceed.send(()).unwrap();
        drop_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
