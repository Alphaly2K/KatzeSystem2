use super::{Pcm, VorbisDecoder};
use crate::io::SharedSource;
use crate::{Error, Result, read};
use std::sync::Arc;

mod stream;
use stream::Stream;

/// 可重复读取的音频来源；流式 Vorbis 先准备四秒采样，再由独立线程按需预取。
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
    Streaming(Arc<Stream>),
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
        let mut decoder = VorbisDecoder::from_shared(source.clone())?;
        let rate = decoder.sample_rate();
        let channels = decoder.channels();
        let frames = decoder.frames();
        let prefill_frames = (u64::from(rate) * 4).min(frames as u64) as usize;
        let prefill = super::pcm::sample_count(prefill_frames, channels)?;
        let mut samples = Vec::new();
        if whole || frames <= prefill_frames {
            super::pcm::sample_count(frames, channels)?;
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
        let stream = Stream::new(channels, frames, samples, source, move |seek| {
            if let Some(frame) = seek {
                decoder.seek_frame(frame)?;
            }
            decoder.read_packet()
        })?;
        Ok(Self {
            rate,
            channels,
            frames,
            data: Data::Streaming(Arc::new(stream)),
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

    /// 显式解码并缓存完整 PCM；整段数据仍受 PCM 容量上限约束。
    pub fn pcm(&self) -> Result<&Pcm> {
        match &self.data {
            Data::Pcm(pcm) => Ok(pcm),
            Data::Streaming(stream) => stream.pcm(),
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
        if output.len() > read::MAX_BYTES / 2
            || !output.len().is_multiple_of(channels)
            || offset
                .checked_add(output.len() / channels)
                .is_none_or(|end| end > self.frames)
        {
            return Err(Error::new(0, "声音来源读取范围、容量或采样帧对齐无效"));
        }
        if output.is_empty() {
            return Ok(true);
        }
        let start = offset * channels;
        match &self.data {
            Data::Pcm(pcm) => {
                output.copy_from_slice(&pcm.samples()[start..start + output.len()]);
                Ok(true)
            }
            Data::Streaming(stream) => stream.read(start, output, wait),
        }
    }

    #[cfg(test)]
    fn prefetched(
        rate: u32,
        channels: u8,
        frames: usize,
        prefix: Vec<i16>,
        next: impl FnMut(Option<usize>) -> Result<Option<Vec<i16>>> + Send + 'static,
    ) -> Result<Self> {
        let stream = Stream::new(channels, frames, prefix, Arc::new(Vec::new()), next)?;
        Ok(Self {
            rate,
            channels,
            frames,
            data: Data::Streaming(Arc::new(stream)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn streaming_stereo_seeks_across_prefetch_windows_without_changing_samples() {
        let bytes = include_bytes!("../../tests/fixtures/audio/stereo-8000-stream.ogg");
        let expected = super::super::decode_vorbis(bytes).unwrap();
        let source = Source::from_vorbis(Arc::new(bytes.to_vec()), false).unwrap();
        for frame in [90000, 33001, 70000, expected.frames() - 257, 0, 64017] {
            let count = (expected.frames() - frame).min(512) * 2;
            let mut output = vec![0; count];
            source.read_frames(frame, &mut output).unwrap();
            assert_eq!(output, expected.samples()[frame * 2..frame * 2 + count]);
        }
        assert_eq!(source.pcm().unwrap(), &expected);
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
                Source::prefetched(1, 2, 4, vec![1, 2, 3, 4], move |_| receiver.recv().unwrap())
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
            let guard = shared.state.buffer.lock().unwrap();
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
                assert_eq!(
                    source.try_read_frames(2, &mut [0; 4]).unwrap_err().offset,
                    19
                );
            } else {
                sender.send(Ok(Some(vec![5, 6, 7, 8]))).unwrap();
                assert_eq!(worker.join().unwrap(), (Ok(()), [5, 6, 7, 8]));
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
        let source = Source::prefetched(1, 1, 4, vec![1, 2], move |_| producer.packet()).unwrap();
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
