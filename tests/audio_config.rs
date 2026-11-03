use katze_system2::audio::{SoundConfig, SoundPart};

fn part(id: i32, system_voice: i32, children: Vec<SoundPart>) -> SoundPart {
    SoundPart {
        name: Vec::new(),
        id,
        volume: id.wrapping_mul(3),
        system_voice,
        children,
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
    assert!(SoundConfig::default().volume_by_name(b"master").is_err());
}
