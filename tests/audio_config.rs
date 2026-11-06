use katze_system2::audio::{SoundConfig, SoundPart};

fn part(id: i32, system_voice: i32, children: Vec<SoundPart>) -> SoundPart {
    SoundPart {
        name: Vec::new(),
        id,
        volume: id.wrapping_mul(3),
        system_voice,
        children,
        ..SoundPart::default()
    }
}

#[test]
fn voice_selection_keeps_tree_order_path_fallback_and_duplicate_weights() {
    let config = SoundConfig {
        parts: vec![
            part(10, 1, vec![part(11, 0, vec![part(111, 1, vec![])])]),
            part(
                20,
                2,
                vec![
                    part(21, 0, vec![]),
                    part(22, -1, vec![part(221, 1, vec![])]),
                ],
            ),
            part(10, 0, vec![]),
        ],
        ..SoundConfig::default()
    };
    assert_eq!(config.system_voice_candidates(&[]).unwrap(), [10, 111, 221]);
    assert_eq!(config.system_voice_enabled(&[20, 21]).unwrap(), 0);
    assert_eq!(config.system_voice_enabled(&[999, 10, 11]).unwrap(), 0);
    assert_eq!(config.system_voice_enabled(&[20, 999, 22]).unwrap(), -1);
    assert_eq!(config.system_voice_enabled(&[10, 11, 999]).unwrap(), 1);
    assert_eq!(config.volume(&[20, 21]).unwrap(), 63);
    assert_eq!(config.volume(&[999, 10, 11]).unwrap(), 33);
    assert_eq!(config.volume(&[20, 999, 22]).unwrap(), 66);
    assert_eq!(config.volume(&[20, 21, 999]).unwrap(), 100);
    assert_eq!(config.volume(&[]).unwrap(), 100);
    assert_eq!(
        config
            .system_voice_candidates(&[
                vec![20],
                vec![20, 21],
                vec![20, 22],
                vec![999],
                vec![20, 999],
                vec![20, 22, 221],
                vec![10],
                vec![10],
            ])
            .unwrap(),
        [20, 22, 999, 999, 221, 10, 10]
    );
}

#[test]
fn named_volumes_search_depth_first_and_keep_master_and_multibyte_names_distinct() {
    let config = SoundConfig {
        master_volume: Some(47),
        parts: vec![
            SoundPart {
                name: b"root".to_vec(),
                ..part(
                    10,
                    0,
                    vec![
                        SoundPart {
                            name: b"dup".to_vec(),
                            ..part(11, 0, vec![])
                        },
                        SoundPart {
                            name: b"\x81\x61X".to_vec(),
                            ..part(12, 0, vec![])
                        },
                    ],
                )
            },
            SoundPart {
                name: b"DUP".to_vec(),
                ..part(20, 0, vec![])
            },
            SoundPart {
                name: b"master".to_vec(),
                ..part(30, 0, vec![])
            },
        ],
        ..SoundConfig::default()
    };
    for (name, expected) in [
        (b"ROOT".as_slice(), 30),
        (b"DuP", 33),
        (b"master", 47),
        (b"MASTER", 90),
        (b"\x81\x61x", 36),
        (b"\x81\x41X", 100),
        (b"root/dup", 100),
        (b"missing", 100),
    ] {
        assert_eq!(config.volume_by_name(name).unwrap(), expected);
    }
    assert_eq!(config.group_volume(b"master").unwrap(), 90);
    assert!(SoundConfig::default().volume_by_name(b"master").is_err());
}

#[test]
fn file_policy_uses_separate_prefixes_parent_priority_and_raw_switches() {
    let mut config = SoundConfig {
        parts: vec![
            SoundPart {
                name: b"category".to_vec(),
                file_prefix: b"AB".to_vec(),
                volume: 37,
                play_enabled: 0,
                system_voice: -1,
                children: vec![SoundPart {
                    file_prefix: b"abcd".to_vec(),
                    volume: 83,
                    ..Default::default()
                }],
                ..Default::default()
            },
            SoundPart {
                file_prefix: b"ab".to_vec(),
                volume: 99,
                ..Default::default()
            },
            SoundPart {
                children: vec![SoundPart {
                    file_prefix: b"\x81\x61X".to_vec(),
                    volume: -37,
                    play_enabled: -1,
                    system_voice: 2,
                    mute: 2,
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    for (name, expected) in [
        (b"abcd.wav".as_slice(), [37, 0, -1, 0]),
        (b"aB\0unused", [37, 0, -1, 0]),
        (b"\x81\x61x-a", [-37, -1, 2, 2]),
        (b"\x81\x41X-a", [100, 1, 1, 100]),
        (b"category", [100, 1, 1, 100]),
        (b"folder/abcd.wav", [100, 1, 1, 100]),
        (b"", [100, 1, 1, 100]),
    ] {
        assert_eq!(
            [
                config.file_volume(name).unwrap(),
                config.file_play_enabled(name).unwrap(),
                config.file_system_voice(name).unwrap(),
                config.file_mute(name).unwrap(),
            ],
            expected
        );
    }
    assert!(config.file_part(&[b'a'; 512]).is_err());
    for (value, expected) in [(-1, 0), (0, 0), (1, 1), (-3, -3)] {
        config.master_mute = value;
        assert_eq!(config.master_mute(), expected);
    }
}

#[test]
fn effective_volume_applies_the_matched_branch_inside_out_with_per_stage_wrapping() {
    let mut config = SoundConfig {
        master_volume: Some(47),
        master_mute: -1,
        parts: vec![SoundPart {
            name: b"parent".to_vec(),
            master_volume: 33,
            volume: 37,
            mute: 0,
            children: vec![SoundPart {
                file_prefix: b"AB".to_vec(),
                master_volume: 65,
                volume: 83,
                mute: 0,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(config.effective_file_volume(b"ab.ogg").unwrap(), 2);
    assert_eq!(config.file_volume(b"ab.ogg").unwrap(), 83);
    assert_eq!(config.effective_file_volume(b"missing").unwrap(), 47);
    config.parts[0].file_prefix = b"AB".to_vec();
    assert_eq!(config.effective_file_volume(b"ab.ogg").unwrap(), 5);
    config.parts[0].file_prefix.clear();
    config.parts[0].mute = 1;
    assert_eq!(config.effective_file_volume(b"ab.ogg").unwrap(), 0);
    config.parts[0].mute = 0;
    config.parts[0].master_volume = 100;
    config.parts[0].volume = 100;
    config.master_volume = Some(100);
    config.parts[0].children[0].master_volume = 9999;
    config.parts[0].children[0].volume = i32::MAX;
    assert_eq!(config.effective_file_volume(b"ab.ogg").unwrap(), -99);
    config.parts[0].children[0].mute = -1;
    assert_eq!(config.file_mute(b"ab.ogg").unwrap(), 0);
    assert_eq!(config.effective_file_volume(b"ab.ogg").unwrap(), 0);
    assert!(config.effective_file_volume(&[b'a'; 512]).is_err());
    config.master_volume = None;
    config.master_mute = 2;
    assert_eq!(config.effective_file_volume(&[b'a'; 512]).unwrap(), 0);
}
