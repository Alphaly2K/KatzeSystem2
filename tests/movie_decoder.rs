use katze_system2::{
    Error, Result,
    io::{ReadSource, SharedSource},
    movie::Decoder,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const CLIP: &[u8] = include_bytes!("fixtures/movie/clip.mpg");

struct ShortReads(&'static [u8], Arc<AtomicUsize>);
impl ReadSource for ShortReads {
    fn len(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        self.1.fetch_max(output.len(), Ordering::Relaxed);
        let bytes = self.0.get(offset as usize..).unwrap_or_default();
        let count = bytes.len().min(output.len()).min(79);
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

#[test]
fn streaming_movie_detects_undeclared_audio_and_preserves_frames_samples_and_eof() {
    let largest = Arc::new(AtomicUsize::new(0));
    let mut decoder = Decoder::new(Arc::new(ShortReads(CLIP, largest.clone()))).unwrap();
    let metadata = decoder.metadata();
    assert_eq!(metadata.size, [32, 32]);
    assert_eq!(metadata.frame_rate, 25.);
    assert_eq!(metadata.sample_rate, 48000);
    assert!(decoder.advance(f64::NAN).is_err());
    assert!(decoder.advance(-1.).is_err());
    assert!(decoder.advance(2.).is_err());
    let result = std::thread::spawn(move || {
        let mut frames = Vec::new();
        let mut samples = Vec::new();
        let mut ended = false;
        for _ in 0..60 {
            let decoded = decoder.advance(1. / 60.).unwrap();
            if let Some(video) = decoded.video {
                frames.push(video.time);
                let red = video.surface.pixel(8, 8).unwrap().to_le_bytes();
                let blue = video.surface.pixel(8, 24).unwrap().to_le_bytes();
                assert!(red[2] > 200 && red[0] < 50 && red[1] < 50);
                assert!(blue[0] > 200 && blue[2] < 50 && blue[1] < 50);
                assert_eq!([red[3], blue[3]], [255; 2]);
            }
            if let Some(audio) = decoded.audio {
                assert!((audio.time - samples.len() as f64 / 2. / 48000.).abs() < 1e-9);
                samples.extend(audio.samples);
            }
            if decoded.ended {
                ended = true;
                break;
            }
        }
        assert!(ended);
        let finished = decoder.advance(1. / 60.).unwrap();
        assert!(finished.ended && finished.video.is_none() && finished.audio.is_none());
        (frames, samples)
    })
    .join()
    .unwrap();
    assert_eq!(result.0, [0., 0.04, 0.08, 0.12]);
    assert_eq!(result.1.len(), 8064 * 2);
    assert!(
        result
            .1
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.)
    );
    assert!(
        result
            .1
            .iter()
            .filter(|&&sample| sample.abs() > 0.01)
            .count()
            > 10000
    );
    assert!(largest.load(Ordering::Relaxed) <= 32768);
}

#[test]
fn mp3_movie_streams_keep_variable_frames_mono_stereo_and_replay_state() {
    for (bytes, mono) in [
        (
            include_bytes!("fixtures/movie/clip-mp3.mpg").as_slice(),
            false,
        ),
        (
            include_bytes!("fixtures/movie/clip-mp3-mono.mpg").as_slice(),
            true,
        ),
    ] {
        let largest = Arc::new(AtomicUsize::new(0));
        let mut decoder = Decoder::new(Arc::new(ShortReads(bytes, largest.clone()))).unwrap();
        let metadata = decoder.metadata();
        assert_eq!(metadata.size, [32, 32]);
        assert_eq!(metadata.frame_rate, 25.);
        assert_eq!(metadata.sample_rate, 44100);
        let mut reference = None;
        for pass in 0..2 {
            let mut frames = Vec::new();
            let mut samples = Vec::new();
            let mut ended = false;
            for _ in 0..120 {
                let decoded = decoder.advance(1. / 60.).unwrap();
                if let Some(video) = decoded.video {
                    frames.push(video.time);
                }
                if let Some(audio) = decoded.audio {
                    assert!((audio.time - samples.len() as f64 / 2. / 44100.).abs() < 1e-9);
                    samples.extend(audio.samples);
                }
                if decoded.ended {
                    ended = true;
                    break;
                }
            }
            assert!(ended);
            assert_eq!(frames, [0., 0.04, 0.08, 0.12]);
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.));
            assert!(samples.iter().filter(|&&s| s.abs() > 0.01).count() > 6000);
            assert_eq!(samples.chunks_exact(2).all(|p| p[0] == p[1]), mono);
            if let Some(reference) = &reference {
                assert_eq!(&samples, reference);
            } else {
                reference = Some(samples);
            }
            if pass == 0 {
                decoder.rewind().unwrap();
            }
        }
        assert!(largest.load(Ordering::Relaxed) <= 32768);
    }
}

struct Broken(u8);
impl ReadSource for Broken {
    fn len(&self) -> Result<u64> {
        Ok(100)
    }
    fn read_at(&self, _: u64, output: &mut [u8]) -> Result<usize> {
        match self.0 {
            0 => Err(Error::new(0, "synthetic read failure")),
            1 => Ok(0),
            2 => Ok(output.len() + 1),
            _ => panic!("synthetic read panic"),
        }
    }
}

#[test]
fn invalid_movie_inputs_and_read_failures_return_diagnostics() {
    for source in [
        Arc::new(Vec::<u8>::new()) as SharedSource,
        Arc::new(vec![0; 100]),
    ] {
        assert!(Decoder::new(source).is_err());
    }
    for kind in 0..4 {
        let error = Decoder::new(Arc::new(Broken(kind))).err().unwrap();
        assert!(
            error.message.contains(if kind == 0 {
                "synthetic read failure"
            } else if kind == 3 {
                "panic"
            } else {
                "电影来源提前结束"
            }),
            "{error}"
        );
    }
}
