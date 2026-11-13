use katze_system2::{
    Result,
    audio::{VorbisDecoder, decode_vorbis, decode_vorbis_source},
    io::ReadSource,
};

const MONO: &[u8] = include_bytes!("fixtures/audio/mono-44100.ogg");
const STEREO: &[u8] = include_bytes!("fixtures/audio/stereo-48000.ogg");

#[test]
fn source_decoding_keeps_pcm_when_random_reads_return_short_chunks() {
    struct Chunked(&'static [u8]);
    impl ReadSource for Chunked {
        fn len(&self) -> Result<u64> {
            ReadSource::len(self.0)
        }
        fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
            let count = output.len().min(13);
            self.0.read_at(offset, &mut output[..count])
        }
    }
    for bytes in [MONO, STEREO] {
        assert_eq!(
            decode_vorbis_source(&Chunked(bytes)).unwrap(),
            decode_vorbis(bytes).unwrap()
        );
    }
}

#[test]
fn vorbis_preserves_channel_order_and_exact_final_frames_against_independent_pcm() {
    for (encoded, reference, rate, channels, frames) in [
        (
            MONO,
            include_bytes!("fixtures/audio/mono-44100.pcm").as_slice(),
            44100,
            1,
            1379,
        ),
        (
            STEREO,
            include_bytes!("fixtures/audio/stereo-48000.pcm").as_slice(),
            48000,
            2,
            1903,
        ),
    ] {
        let pcm = decode_vorbis(encoded).unwrap();
        assert_eq!(pcm.sample_rate(), rate);
        assert_eq!(pcm.channels(), channels);
        assert_eq!(pcm.frames(), frames);
        assert_eq!(pcm.samples().len() * 2, reference.len());
        let mut decoder = VorbisDecoder::new(encoded).unwrap();
        assert_eq!(decoder.sample_rate(), rate);
        assert_eq!(decoder.channels(), channels);
        assert_eq!(decoder.frames(), frames);
        let mut packets = Vec::new();
        let mut packet_count = 0;
        while let Some(packet) = decoder.read_packet().unwrap() {
            assert!(!packet.is_empty());
            assert!(packet.len().is_multiple_of(channels as usize));
            packets.extend(packet);
            packet_count += 1;
        }
        assert!(packet_count > 1);
        assert_eq!(packets, pcm.samples());
        assert_eq!(decoder.read_packet().unwrap(), None);
        for (index, (actual, expected)) in pcm
            .samples()
            .iter()
            .zip(reference.chunks_exact(2))
            .enumerate()
        {
            let expected = i16::from_le_bytes(expected.try_into().unwrap());
            assert!(
                (i32::from(*actual) - i32::from(expected)).abs() <= 2,
                "sample {index}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn packet_reader_reports_corruption_when_reading_the_audio_page() {
    let mut corrupt = MONO.to_vec();
    *corrupt.last_mut().unwrap() ^= 1;
    let mut decoder = VorbisDecoder::new(corrupt.as_slice()).unwrap();
    assert_eq!(decoder.frames(), 1379);
    loop {
        match decoder.read_packet() {
            Ok(Some(_)) => (),
            Err(_) => break,
            Ok(None) => panic!("corrupt audio page was accepted"),
        }
    }
}

#[test]
fn shared_source_and_partial_decoder_state_move_together_to_a_worker() {
    for bytes in [MONO, STEREO] {
        let source: katze_system2::io::SharedSource = std::sync::Arc::new(bytes.to_vec());
        let mut decoder = VorbisDecoder::from_shared(source.clone()).unwrap();
        let mut actual = decoder.read_packet().unwrap().unwrap();
        drop(source);
        let remaining = std::thread::spawn(move || {
            let mut samples = Vec::new();
            while let Some(packet) = decoder.read_packet().unwrap() {
                samples.extend(packet);
            }
            samples
        })
        .join()
        .unwrap();
        actual.extend(remaining);
        assert_eq!(actual, decode_vorbis(bytes).unwrap().samples());
    }
}

#[test]
fn packet_seeks_preserve_exact_samples_across_pages_and_end_of_stream() {
    fn check<S: katze_system2::io::ReadSource + ?Sized>(
        bytes: &[u8],
        mut decoder: VorbisDecoder<'_, S>,
    ) {
        let expected = decode_vorbis(bytes).unwrap();
        let channels = expected.channels() as usize;
        let mut targets = vec![0, 1, expected.frames() - 1, expected.frames()];
        let mut at = 0;
        while at < bytes.len() {
            let end = u64::from_le_bytes(bytes[at + 6..at + 14].try_into().unwrap());
            if end > 0 && end < expected.frames() as u64 {
                targets.extend([end as usize - 1, end as usize, end as usize + 1]);
            }
            let segments = bytes[at + 26] as usize;
            at += 27
                + segments
                + bytes[at + 27..at + 27 + segments]
                    .iter()
                    .map(|count| *count as usize)
                    .sum::<usize>();
        }
        targets.sort_unstable();
        targets.dedup();
        let reverse = targets.iter().rev().copied().collect::<Vec<_>>();
        for frame in targets.into_iter().chain(reverse) {
            decoder.seek_frame(frame).unwrap();
            assert!(decoder.seek_frame(expected.frames() + 1).is_err());
            let count = (expected.frames() - frame).min(257) * channels;
            let mut actual = Vec::new();
            while actual.len() < count {
                actual.extend(decoder.read_packet().unwrap().unwrap());
            }
            actual.truncate(count);
            assert_eq!(
                actual,
                expected.samples()[frame * channels..frame * channels + count],
                "frame {frame}"
            );
            if frame == expected.frames() {
                assert!(decoder.read_packet().unwrap().is_none());
            }
        }
    }
    let long = include_bytes!("fixtures/audio/mono-8000-long.ogg");
    let stereo_stream = include_bytes!("fixtures/audio/stereo-8000-stream.ogg");
    for bytes in [MONO, STEREO, long.as_slice(), stereo_stream.as_slice()] {
        check(bytes, VorbisDecoder::new(bytes).unwrap());
    }
    check(
        long,
        VorbisDecoder::from_shared(std::sync::Arc::new(long.to_vec())).unwrap(),
    );
}

#[test]
fn vorbis_rejects_incomplete_pages_corruption_chained_streams_and_excessive_frame_counts() {
    let pages: Vec<_> = MONO
        .windows(4)
        .enumerate()
        .filter_map(|(index, magic)| (magic == b"OggS").then_some(index))
        .collect();
    let last = *pages.last().unwrap();
    for end in [0, 4, 26, pages[1], last, last + 27, MONO.len() - 1] {
        assert!(decode_vorbis(&MONO[..end]).is_err(), "prefix length {end}");
    }
    let mut corrupt = MONO.to_vec();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(decode_vorbis(&corrupt).is_err());
    assert!(decode_vorbis(&[MONO, STEREO].concat()).is_err());
    let mut excessive = MONO.to_vec();
    excessive[last + 6..last + 14].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(decode_vorbis(&excessive).is_err());
}
