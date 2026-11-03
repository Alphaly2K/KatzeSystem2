use katze_system2::audio::decode_vorbis;

const MONO: &[u8] = include_bytes!("fixtures/audio/mono-44100.ogg");
const STEREO: &[u8] = include_bytes!("fixtures/audio/stereo-48000.ogg");

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
