use super::{Data, Pcm, Source, Stream, VorbisDecoder};
use crate::Result;
use crate::host::storage::{ReadSnapshot, SharedSource};
use std::{collections::VecDeque, sync::Arc};

const BUDGET: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub(in crate::audio) struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

struct Entry {
    key: ReadSnapshot,
    whole: bool,
    prepared: Prepared,
}

enum Prepared {
    Pcm(Pcm),
    Prefix {
        rate: u32,
        channels: u8,
        frames: usize,
        samples: Arc<[i16]>,
    },
}

impl Cache {
    pub(in crate::audio) fn load(&mut self, source: SharedSource, whole: bool) -> Result<Source> {
        let Some(key) = source.snapshot() else {
            return Source::from_vorbis(source, whole);
        };
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.key == key && entry.whole == whole)
        {
            let prepared = self.entries.remove(index).unwrap();
            let result = prepared.prepared.open(source);
            self.entries.push_front(prepared);
            return result;
        }
        let loaded = Source::from_vorbis(source, whole)?;
        let prepared = Prepared::capture(&loaded);
        let bytes = prepared.bytes();
        if (1..=BUDGET).contains(&bytes) {
            while self.bytes + bytes > BUDGET {
                self.bytes -= self.entries.pop_back().unwrap().prepared.bytes();
            }
            self.bytes += bytes;
            self.entries.push_front(Entry {
                key,
                whole,
                prepared,
            });
        }
        Ok(loaded)
    }
}

impl Prepared {
    fn capture(source: &Source) -> Self {
        match &source.data {
            Data::Pcm(pcm) => Self::Pcm(pcm.clone()),
            Data::Streaming(stream) => Self::Prefix {
                rate: source.rate,
                channels: source.channels,
                frames: source.frames,
                samples: stream.state.prefix.clone(),
            },
        }
    }

    fn bytes(&self) -> usize {
        let samples = match self {
            Self::Pcm(pcm) => pcm.samples().len(),
            Self::Prefix { samples, .. } => samples.len(),
        };
        samples * std::mem::size_of::<i16>()
    }

    fn open(&self, source: SharedSource) -> Result<Source> {
        match self {
            Self::Pcm(pcm) => Ok(pcm.clone().into()),
            Self::Prefix {
                rate,
                channels,
                frames,
                samples,
            } => {
                let first = samples.len() / *channels as usize;
                let mut decoder = None;
                let pending = source.clone();
                let stream =
                    Stream::new(*channels, *frames, samples.clone(), source, move |seek| {
                        // 缓存只持有初始采样；每个预取窗口独立进入余下的流。
                        if decoder.is_none() {
                            let mut reader = VorbisDecoder::from_shared(pending.clone())?;
                            reader.seek_frame(seek.unwrap_or(first))?;
                            decoder = Some(reader);
                        } else if let (Some(reader), Some(frame)) = (&mut decoder, seek) {
                            reader.seek_frame(frame)?;
                        }
                        decoder.as_mut().unwrap().read_packet()
                    })?;
                Ok(Source {
                    rate: *rate,
                    channels: *channels,
                    frames: *frames,
                    data: Data::Streaming(Arc::new(stream)),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::storage::ReadSource;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Observed {
        key: Option<ReadSnapshot>,
        bytes: &'static [u8],
        reads: AtomicUsize,
    }

    impl ReadSource for Observed {
        fn len(&self) -> Result<u64> {
            Ok(self.bytes.len() as u64)
        }

        fn snapshot(&self) -> Option<ReadSnapshot> {
            self.key.clone()
        }

        fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.bytes.read_at(offset, output)
        }
    }

    fn observed(bytes: &'static [u8], key: Option<ReadSnapshot>) -> Arc<Observed> {
        Arc::new(Observed {
            bytes,
            key,
            reads: AtomicUsize::new(0),
        })
    }

    #[test]
    fn cached_pcm_is_only_reused_for_the_same_snapshot() {
        let mono = include_bytes!("../../../tests/fixtures/audio/mono-44100.ogg");
        let stereo = include_bytes!("../../../tests/fixtures/audio/stereo-48000.ogg");
        let mut cache = Cache::default();
        let key = ReadSnapshot::new(Arc::new(()), 1);
        let first = cache.load(observed(mono, Some(key.clone())), true).unwrap();
        let reopened = observed(mono, Some(key));
        let second = cache.load(reopened.clone(), true).unwrap();
        assert_eq!(second.pcm().unwrap(), first.pcm().unwrap());
        assert_eq!(reopened.reads.load(Ordering::Relaxed), 0);
        for current in [
            observed(stereo, Some(ReadSnapshot::new(Arc::new(()), 1))),
            observed(stereo, None),
        ] {
            let out = cache.load(current.clone(), true).unwrap();
            assert_eq!(out.channels(), 2);
            assert!(current.reads.load(Ordering::Relaxed) > 0);
        }
    }

    #[test]
    fn cached_prefix_keeps_independent_windows_and_exact_samples_after_the_boundary() {
        let bytes = include_bytes!("../../../tests/fixtures/audio/stereo-8000-stream.ogg");
        let expected = crate::audio::decode_vorbis(bytes).unwrap();
        let mut cache = Cache::default();
        let key = ReadSnapshot::new(Arc::new(()), 1);
        let first = cache
            .load(observed(bytes, Some(key.clone())), false)
            .unwrap();
        let second = cache.load(observed(bytes, Some(key)), false).unwrap();
        let (Data::Streaming(a), Data::Streaming(b)) = (&first.data, &second.data) else {
            panic!()
        };
        assert!(!Arc::ptr_eq(a, b));
        assert!(Arc::ptr_eq(&a.state.prefix, &b.state.prefix));
        let after_prefix = b.state.prefix.len() / 2;
        for (source, frame) in [
            (&first, 81_000),
            (&second, after_prefix - 20),
            (&second, 91_000),
            (&first, after_prefix + 600),
        ] {
            let mut buffer = [0; 128];
            source.read_frames(frame, &mut buffer).unwrap();
            assert_eq!(buffer, expected.samples()[frame * 2..frame * 2 + 128]);
        }
    }
}
