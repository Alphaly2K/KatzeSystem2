use katze_system2::{
    Error,
    fes::StringState,
    message::{MessageDefaults, TextEncoding},
};

fn word(state: &StringState, offset: usize) -> u32 {
    u32::from_le_bytes(
        state.configuration_bytes()[offset..offset + 4]
            .try_into()
            .unwrap(),
    )
}

#[test]
fn construction_keeps_fes_colors_and_raw_font_parameters_separate_from_message_defaults() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let defaults = MessageDefaults {
            font_name: "Initial Face".encode_utf16().collect(),
            bold: true,
            shadow: 9,
            outline: 8,
            glyph_parameter: Some(17),
            reveal: true,
        };
        let state = StringState::new(encoding, &defaults, 7).unwrap();
        let extra = usize::from(encoding == TextEncoding::Unicode) * 0x40;
        assert_eq!(word(&state, 0), if extra == 0 { 0xe8 } else { 0x12c });
        assert_eq!(word(&state, 4), 1);
        assert_eq!([word(&state, 8), word(&state, 12)], [800, 600]);
        assert_eq!(
            [word(&state, 16), word(&state, 20), word(&state, 24)],
            [0; 3]
        );
        assert_eq!(state.restore_colors(), [u32::MAX, 0xff00_0000, u32::MAX]);
        assert_eq!(word(&state, 0x80), 7);
        assert_eq!(word(&state, 0xc4 + extra), 700);
        assert_eq!(
            [word(&state, 0xc8 + extra), word(&state, 0xcc + extra)],
            [9, 8]
        );
        assert_eq!(word(&state, 0xd0 + extra), if extra == 0 { 17 } else { 0 });
        assert!(!state.configuration_dirty());
    }
}

#[test]
fn layout_preserves_legacy_view_flags_color_packing_and_atomic_parameter_errors() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let mut state = StringState::new(encoding, &MessageDefaults::default(), 0).unwrap();
        state
            .apply_layout(b"view", |index, _| Ok([-9, 17][index]), 37)
            .unwrap();
        assert_eq!(state.base.base.size(), [-9, 17]);
        assert_eq!([word(&state, 8), word(&state, 12)], [u32::MAX - 8, 17]);
        assert_eq!(state.configuration_dirty(), encoding == TextEncoding::Cp932);
        if encoding == TextEncoding::Unicode {
            assert_eq!(word(&state, 0xec), 1);
        }
        state.clear_configuration_dirty();
        state.apply_layout(b"weight", |_, _| Ok(551), 37).unwrap();
        assert!(!state.configuration_dirty());
        state
            .apply_layout(b"frame", |_, default| Ok(default), 37)
            .unwrap();
        assert_eq!([word(&state, 0x74), word(&state, 0x78)], [37; 2]);
        state
            .apply_layout(
                b"size",
                |index, default| Ok(if index == 0 { 31 } else { default }),
                0,
            )
            .unwrap();
        assert_eq!(
            (0..5)
                .map(|i| word(&state, 0x1c + i * 4))
                .collect::<Vec<_>>(),
            [31; 5]
        );
        let components = [257, -1, 16, 2, 3, 4, 5, 6, 7];
        state
            .apply_layout(b"color", |index, _| Ok(components[index]), 0)
            .unwrap();
        assert_eq!(
            state.restore_colors(),
            [0xff10_ff01, 0xff04_0302, 0xff07_0605]
        );
        assert_eq!(
            [word(&state, 16), word(&state, 20), word(&state, 24)],
            state.restore_colors()
        );
        let snapshot = state.clone();
        assert!(
            state
                .apply_layout(
                    b"move",
                    |index, _| if index == 0 {
                        Ok(123)
                    } else {
                        Err(Error::new(0, "synthetic expression failure"))
                    },
                    0
                )
                .is_err()
        );
        assert_eq!(state, snapshot);
        assert!(
            !state
                .apply_layout(
                    b"unknown",
                    |_, _| panic!("unknown commands do not evaluate parameters"),
                    0
                )
                .unwrap()
        );
        let font: Vec<_> = "Another Face".encode_utf16().collect();
        state.set_font(&font, 3).unwrap();
        assert!(state.configuration_dirty());
        let snapshot = state.clone();
        assert!(state.set_font(&[b'X' as u16; 64], 1).is_err());
        assert_eq!(state, snapshot);
        state
            .refresh_font_defaults(&MessageDefaults {
                font_name: "Global Face".encode_utf16().collect(),
                bold: true,
                shadow: 5,
                outline: 6,
                glyph_parameter: Some(19),
                ..Default::default()
            })
            .unwrap();
        let font_offset = if encoding == TextEncoding::Cp932 {
            0xc4
        } else {
            0x104
        };
        assert_eq!(word(&state, font_offset), 551);
        assert_eq!(
            [word(&state, font_offset + 4), word(&state, font_offset + 8)],
            [5, 6]
        );
        assert_eq!(word(&state, font_offset + 12), 0);
        assert_eq!(
            state.font_inheritance(),
            [encoding == TextEncoding::Cp932, false, false, true]
        );
        state.clear_configuration_dirty();
        state
            .apply_layout(b"weight", |_, default| Ok(default), 0)
            .unwrap();
        assert_eq!(word(&state, font_offset), 700);
        assert!(!state.configuration_dirty());
        assert!(state.font_inheritance()[2]);
    }
}
