use katze_system2::{
    audio::SoundState,
    io::SharedSource,
    movie::{Decoder, Player},
};
use std::sync::Arc;

fn source() -> SharedSource {
    Arc::new(include_bytes!("fixtures/movie/clip.mpg").to_vec())
}

#[test]
fn movie_clock_follows_audio_and_restarts_full_passes_without_touching_sound_channels() {
    let mut reference = Decoder::new(source()).unwrap();
    let mut expected = Vec::new();
    loop {
        let decoded = reference.advance(1. / 60.).unwrap();
        if let Some(audio) = decoded.audio {
            expected.extend(audio.samples);
        }
        if decoded.ended {
            break;
        }
    }
    reference.rewind().unwrap();
    let first = reference.advance(1. / 60.).unwrap();
    assert_eq!(first.video.unwrap().time, 0.);
    assert_eq!(first.audio.unwrap().time, 0.);
    let mut player = Player::new(source()).unwrap();
    let mut sound = SoundState::default();
    sound.set_movie_output(Some(player.audio_output()));
    for count in [1, 2] {
        assert!(player.play(count).unwrap());
        assert!(!player.play(1).unwrap());
        assert_eq!(player.take_video().unwrap().unwrap().time, 0.);
        let expected: Vec<_> = expected
            .iter()
            .copied()
            .cycle()
            .take(expected.len() * count as usize)
            .collect();
        let mut actual = vec![0.; expected.len() + 100];
        sound.mix_stereo(48000, &mut actual).unwrap();
        assert!(!player.is_playing().unwrap());
        assert!((player.time() - count as f64 * 8064. / 48000.).abs() < 1e-8);
        let differences: Vec<_> = actual[..expected.len()]
            .iter()
            .zip(&expected)
            .enumerate()
            .filter(|(_, (a, b))| (*a - *b).abs() >= 1e-5)
            .take(8)
            .collect();
        assert!(differences.is_empty(), "count {count}: {differences:?}");
        assert!(actual[actual.len() - 100..].iter().all(|&s| s == 0.));
        for channel in 0..64 {
            assert!(!sound.channel(channel).unwrap().is_playing());
        }
    }
    for count in [0, u32::MAX] {
        assert!(player.play(count).unwrap());
        let mut actual = vec![0.; expected.len() * 3];
        sound.mix_stereo(48000, &mut actual).unwrap();
        assert!(player.is_playing().unwrap());
        assert!(player.stop().unwrap());
        actual.fill(0.25);
        sound.mix_stereo(48000, &mut actual).unwrap();
        assert!(actual.iter().all(|&s| s == 0.));
    }
}

#[test]
fn connected_mixer_consumes_movie_audio_with_volume_and_continuous_resampling() {
    for bytes in [
        include_bytes!("fixtures/movie/clip.mpg").as_slice(),
        include_bytes!("fixtures/movie/clip-mp3.mpg").as_slice(),
    ] {
        for rate in [44100, 48000] {
            let mut player = Player::new(Arc::new(bytes.to_vec())).unwrap();
            let mut sound = SoundState::default();
            let mut mixer = sound.connect_output().unwrap();
            sound.set_movie_output(Some(player.audio_output()));
            player.set_volume(0);
            player.play(1).unwrap();
            // 先消费两种编码片段的起始延迟，再检查恢复音量后的输出。
            let mut muted = [0.; 4096];
            mixer.render(rate, &mut muted).unwrap();
            assert!(muted.iter().all(|&sample| sample == 0.));
            assert!(player.time() > 0.);
            player.set_volume(50);
            let mut output = [0.; 128];
            mixer.render(rate, &mut output).unwrap();
            assert!(output.iter().any(|&sample| sample.abs() > 0.01));
            assert!(output.iter().all(|sample| sample.abs() < 0.2));
            player.stop().unwrap();
            output.fill(1.);
            mixer.render(rate, &mut output).unwrap();
            assert!(output.iter().all(|&sample| sample == 0.));
        }
    }
}
