use super::super::{Pcm, decode_vorbis_source, pcm::sample_count};
use crate::io::SharedSource;
use crate::{Error, Result};
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, Thread},
};

pub(super) struct Stream {
    pub(super) state: Arc<Shared>,
}

pub(super) struct Shared {
    pub(super) prefix: Arc<[i16]>,
    source: SharedSource,
    channels: u8,
    frames: usize,
    pub(super) buffer: Mutex<Buffer>,
    changed: Condvar,
    complete: OnceLock<Result<Pcm>>,
    cancelled: AtomicBool,
    rewind: AtomicBool,
    worker: OnceLock<Thread>,
}

pub(super) struct Buffer {
    chunks: VecDeque<(usize, Arc<[i16]>)>,
    start: usize,
    end: usize,
    wanted: Range<usize>,
    capacity: usize,
    error: Option<Error>,
}

impl Shared {
    fn wake(&self) {
        if let Some(worker) = self.worker.get() {
            worker.unpark();
        }
    }
}

impl Stream {
    pub(super) fn new(
        channels: u8,
        frames: usize,
        prefix: Vec<i16>,
        source: SharedSource,
        next: impl FnMut(Option<usize>) -> Result<Option<Vec<i16>>> + Send + 'static,
    ) -> Result<Self> {
        let end = prefix.len();
        let state = Arc::new(Shared {
            prefix: prefix.into(),
            source,
            channels,
            frames,
            buffer: Mutex::new(Buffer {
                chunks: VecDeque::new(),
                start: end,
                end,
                wanted: end..end,
                capacity: end,
                error: None,
            }),
            changed: Condvar::new(),
            complete: OnceLock::new(),
            cancelled: AtomicBool::new(false),
            rewind: AtomicBool::new(false),
            worker: OnceLock::new(),
        });
        let worker_state = state.clone();
        let handle = thread::Builder::new()
            .name("katze-audio-decode".into())
            .spawn(move || {
                let _ = worker_state.worker.set(thread::current());
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prefetch(&worker_state, next)
                }))
                .unwrap_or_else(|_| Err(Error::new(0, "声音预取线程异常退出")));
                if let Err(error) = result {
                    let mut buffer = worker_state
                        .buffer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    buffer.error = Some(error);
                    worker_state.changed.notify_all();
                }
            })
            .map_err(|error| Error::new(0, format!("声音预取线程创建失败：{error}")))?;
        let _ = state.worker.set(handle.thread().clone());
        Ok(Self { state })
    }

    pub(super) fn pcm(&self) -> Result<&Pcm> {
        let shared = &self.state;
        let result = shared.complete.get_or_init(|| {
            sample_count(shared.frames, shared.channels)?;
            decode_vorbis_source(shared.source.as_ref())
        });
        shared.wake();
        result.as_ref().map_err(Clone::clone)
    }

    pub(super) fn read(&self, start: usize, output: &mut [i16], wait: bool) -> Result<bool> {
        let shared = &self.state;
        let end = start + output.len();
        let copy = |pcm: &Pcm, output: &mut [i16]| {
            output.copy_from_slice(&pcm.samples()[start..end]);
            Ok(true)
        };
        if end <= shared.prefix.len() {
            output.copy_from_slice(&shared.prefix[start..end]);
            shared.rewind.store(true, Ordering::Release);
            shared.wake();
            return Ok(true);
        }
        let needed = start.max(shared.prefix.len())..end;
        loop {
            if let Some(Ok(pcm)) = shared.complete.get() {
                return copy(pcm, output);
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
            if let Some(Ok(pcm)) = shared.complete.get() {
                return copy(pcm, output);
            }
            if let Some(error) = &buffer.error {
                return Err(error.clone());
            }
            if needed.start >= buffer.start && needed.end <= buffer.end {
                let first = start.min(shared.prefix.len());
                let prefix_count = shared.prefix.len().saturating_sub(first).min(output.len());
                output[..prefix_count].copy_from_slice(&shared.prefix[first..first + prefix_count]);
                let mut at = needed.start;
                for (base, chunk) in &buffer.chunks {
                    let chunk_end = base + chunk.len();
                    if chunk_end <= at {
                        continue;
                    }
                    let count = (chunk_end - at).min(end - at);
                    output[at - start..at - start + count]
                        .copy_from_slice(&chunk[at - base..at - base + count]);
                    at += count;
                    if at == end {
                        break;
                    }
                }
                buffer.wanted = end..end;
                buffer.capacity = shared.prefix.len();
                shared.wake();
                return Ok(true);
            }
            buffer.wanted = needed.clone();
            buffer.capacity = shared.prefix.len().max(needed.len());
            shared.wake();
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

impl Drop for Stream {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.wake();
    }
}

fn prefetch(
    shared: &Shared,
    mut next: impl FnMut(Option<usize>) -> Result<Option<Vec<i16>>>,
) -> Result<()> {
    let channels = shared.channels as usize;
    let total = shared.frames * channels;
    let mut decoder_at = shared.prefix.len();
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut buffer = shared.buffer.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(shared.complete.get(), Some(Ok(_))) {
            buffer.chunks.clear();
            shared.changed.notify_all();
            return Ok(());
        }
        if shared.rewind.swap(false, Ordering::AcqRel) && buffer.start > shared.prefix.len() {
            buffer.wanted = shared.prefix.len()..shared.prefix.len();
            buffer.capacity = shared.prefix.len();
        }
        let wanted = buffer.wanted.clone();
        if wanted.start < buffer.start || wanted.start > buffer.end {
            buffer.chunks.clear();
            buffer.start = wanted.start;
            buffer.end = wanted.start;
        }
        while buffer
            .chunks
            .front()
            .is_some_and(|(at, chunk)| at + chunk.len() <= wanted.start)
        {
            buffer.chunks.pop_front();
        }
        buffer.start = buffer.chunks.front().map_or(buffer.end, |(at, _)| *at);
        let target = wanted
            .start
            .saturating_add(buffer.capacity)
            .max(wanted.end)
            .min(total);
        if buffer.end >= target {
            drop(buffer);
            thread::park();
            continue;
        }
        let at = buffer.end;
        let seek = (decoder_at != at).then_some(at / channels);
        drop(buffer);
        let Some(packet) = next(seek)? else {
            return Err(Error::new(0, "声音预取采样不足声明长度"));
        };
        if packet.is_empty()
            || !packet.len().is_multiple_of(channels)
            || at.checked_add(packet.len()).is_none_or(|end| end > total)
        {
            return Err(Error::new(0, "声音预取采样数量或帧对齐无效"));
        }
        decoder_at = at + packet.len();
        let mut buffer = shared.buffer.lock().unwrap_or_else(|e| e.into_inner());
        buffer.chunks.push_back((at, packet.into()));
        buffer.end = decoder_at;
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::source::{Data, Source};
    use std::sync::{atomic::AtomicUsize, mpsc};

    #[test]
    fn device_starvation_preserves_samples_resampling_and_other_voices() {
        use crate::audio::{SoundState, playback::Playback};
        for rate in [10, 7, 16] {
            let samples: Vec<i16> = (8192..8275).collect();
            let (sender, receiver) = mpsc::channel();
            let source = Source::prefetched(10, 1, 83, samples[..40].to_vec(), move |_| {
                receiver.recv().unwrap()
            })
            .unwrap();
            let build = |source: Source| {
                let mut state = SoundState::default();
                let sound = &mut state.channels[0];
                sound.playback = Playback::new(&source, false, 0).unwrap();
                sound.playback.start(&source, 0, [5, 60]).unwrap();
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
            let mut first = [0.0; 256];
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
            actual.start(0, 0, 0).unwrap();
            mixer.render(rate, &mut [0.0; 4]).unwrap();
            assert_eq!(actual.channel(0).unwrap().position_frames(), position);
            sender.send(Ok(Some(samples[40..].to_vec()))).unwrap();
            source.read_frames(40, &mut [0]).unwrap();
            let Data::Streaming(stream) = &source.data else {
                unreachable!()
            };
            let _ = stream
                .state
                .complete
                .set(Ok(Pcm::new(10, 1, samples.clone()).unwrap()));
            stream.state.wake();
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
    fn large_streams_seek_reuse_bounded_storage_and_keep_pcm_queries_independent() {
        let frames = crate::read::MAX_BYTES + 100;
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let sample = |at: usize| (at % 30000) as i16;
        let mut cursor = 40;
        let source =
            Source::prefetched(10, 2, frames, (0..80).map(sample).collect(), move |seek| {
                assert!(
                    seen.fetch_add(1, Ordering::Relaxed) < 512,
                    "prefetch ran beyond its window"
                );
                if let Some(frame) = seek {
                    cursor = frame;
                }
                let count = (frames - cursor).min(4);
                if count == 0 {
                    return Ok(None);
                }
                let packet = (cursor * 2..(cursor + count) * 2).map(sample).collect();
                cursor += count;
                Ok(Some(packet))
            })
            .unwrap();
        let mut pending = [91; 18];
        assert!(!source.try_read_frames(10000, &mut pending).unwrap());
        assert_eq!(pending, [91; 18]);
        for at in [10000, 10004, 20000, 68, frames - 9, 52, 0] {
            let mut output = [0; 18];
            source.read_frames(at, &mut output).unwrap();
            assert_eq!(
                output.as_slice(),
                (at * 2..at * 2 + 18).map(sample).collect::<Vec<_>>()
            );
            let Data::Streaming(stream) = &source.data else {
                unreachable!()
            };
            let buffer = stream.state.buffer.lock().unwrap();
            assert!(
                buffer
                    .chunks
                    .iter()
                    .map(|(_, chunk)| chunk.len())
                    .sum::<usize>()
                    <= 80 + 16
            );
        }
        assert!(source.pcm().is_err());
        let mut large = [0; 400];
        source.read_frames(2000, &mut large).unwrap();
        assert_eq!(
            large.as_slice(),
            (4000..4400).map(sample).collect::<Vec<_>>()
        );
        source.read_frames(52, &mut [0; 18]).unwrap();
        let Data::Streaming(stream) = &source.data else {
            unreachable!()
        };
        let buffer = stream.state.buffer.lock().unwrap();
        assert!(
            buffer
                .chunks
                .iter()
                .map(|(_, chunk)| chunk.len())
                .sum::<usize>()
                <= 400 + 16
        );
    }

    #[test]
    fn dropping_a_source_wakes_a_parked_prefetch_worker() {
        struct Marker(std::sync::mpsc::Sender<()>);
        impl Marker {
            fn keep(&self) {}
        }
        impl Drop for Marker {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (done, receiver) = std::sync::mpsc::channel();
        let marker = Marker(done);
        let source = Source::prefetched(10, 1, 1000, vec![1; 40], move |_| {
            marker.keep();
            Ok(Some(vec![1; 4]))
        })
        .unwrap();
        source.read_frames(60, &mut [0; 4]).unwrap();
        drop(source);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    }
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    use crate::audio::source::Data;
    use crate::audio::{Source, playback::Playback};

    #[test]
    fn a_pending_refill_keeps_the_tail_while_seeking_to_a_distant_loop_start() {
        let frames = 1003;
        let sample = |at: usize| (at + 1) as i16;
        let reference: Source = Pcm::new(10, 1, (0..frames).map(sample).collect())
            .unwrap()
            .into();
        let mut expected = Playback::new(&reference, false, 0).unwrap();
        expected.start(&reference, 2, [60, 1001]).unwrap();
        let mut expected_output = [0; 4000];
        let expected_count = expected
            .read(&reference, &mut expected_output, true)
            .unwrap();
        assert!(!expected.playing);
        let mut cursor = 40;
        let source =
            Source::prefetched(10, 1, frames, (0..40).map(sample).collect(), move |seek| {
                if let Some(frame) = seek {
                    cursor = frame;
                }
                let count = (frames - cursor).min(7);
                let packet = (cursor..cursor + count).map(sample).collect();
                cursor += count;
                Ok(Some(packet))
            })
            .unwrap();
        let mut actual = Playback::new(&source, false, 0).unwrap();
        actual.start(&source, 2, [60, 1001]).unwrap();
        let mut output = Vec::new();
        for _ in 0..1000 {
            let mut chunk = [0; 37];
            let count = actual.read(&source, &mut chunk, false).unwrap();
            output.extend_from_slice(&chunk[..count]);
            if !actual.playing {
                break;
            }
            if count == chunk.len() {
                continue;
            }
            let Data::Streaming(stream) = &source.data else {
                unreachable!()
            };
            let shared = &stream.state;
            let mut buffer = shared.buffer.lock().unwrap();
            let wanted = buffer.wanted.clone();
            while buffer.start > wanted.start || buffer.end < wanted.end {
                assert!(buffer.error.is_none());
                let (guard, timeout) = shared
                    .changed
                    .wait_timeout(buffer, std::time::Duration::from_secs(5))
                    .unwrap();
                assert!(
                    !timeout.timed_out(),
                    "prefetch did not prepare the requested range"
                );
                buffer = guard;
            }
        }
        assert!(!actual.playing, "playback remained at the loop boundary");
        assert_eq!(output, expected_output[..expected_count]);
    }
}
