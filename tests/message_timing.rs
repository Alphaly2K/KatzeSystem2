use katze_system2::message::{
    AtomKind, BreakCharacters, Layout, LayoutSettings, Markup, Schedule, StyleDefaults,
    TextEncoding, TimingSettings, VerticalAlignment,
};

fn timing(encoding: TextEncoding) -> TimingSettings {
    TimingSettings {
        encoding,
        time_scale: Some(10),
        character_delay: 3,
        period_delay: 8,
        skip_spaces: false,
    }
}

fn prepare(text: &str, encoding: TextEncoding, width: i32) -> (Markup, Layout) {
    let markup = Markup::parse(
        text,
        &StyleDefaults {
            text_sizes: [20; 5],
            ruby_sizes: [12; 5],
            text_color: 0xff00_0000,
            ruby_color: 0xff00_0000,
            shadow_color: 0,
        },
    )
    .unwrap();
    let layout = markup
        .layout(
            &LayoutSettings {
                encoding,
                width,
                margins: [1, 3, 2],
                offset: [0; 2],
                character_spacing: 2,
                line_spacing: 2,
                normal_size: 20,
                ruby_offset: -1,
                vertical_alignment: VerticalAlignment::Preserve,
                line_breaking: true,
                word_wrap: false,
                hanging_indent: false,
                break_characters: BreakCharacters::for_encoding(encoding),
            },
            |ch, size| Ok(size as i32 / 2 + ch as i32 % 3),
        )
        .unwrap();
    (markup, layout)
}

fn schedule(text: &str, settings: &TimingSettings, initial: u32) -> Schedule {
    let (markup, layout) = prepare(text, settings.encoding, 1000);
    markup.schedule(&layout, settings, initial).unwrap()
}

#[test]
fn waits_add_replace_and_reset_units_without_requiring_monotonic_times() {
    let mut config = timing(TextEncoding::Cp932);
    config.time_scale = Some(1000);
    let result = schedule(r"A\wf10;B\w10;C\wrD", &config, 0);
    assert_eq!(result.deadlines, [0, 1300, 0, 0, 0]);
    assert_eq!(result.accumulated_units, 3);

    for (scale, deadlines, units) in [
        (0, [0, 0], 15),
        (10, [9, 30], 33),
        (u32::MAX, [429496728, 429496728], 15),
    ] {
        config.time_scale = Some(scale);
        let result = schedule(r"A\w10;B", &config, 9);
        assert_eq!(result.deadlines, deadlines);
        assert_eq!(result.accumulated_units, units);
    }

    config.time_scale = Some(1);
    config.character_delay = u32::MAX;
    config.period_delay = 0x8000_0001;
    let result = schedule("A.。B", &config, u32::MAX - 1);
    assert_eq!(
        result.deadlines,
        [429496729, 429496729, 429496729, 214748364]
    );
    assert_eq!(result.accumulated_units, 0x7fff_fffc);
}

#[test]
fn periods_and_spaces_advance_the_configured_encoding_path() {
    for (encoding, deadlines, units) in [
        (TextEncoding::Cp932, [0, 3, 6, 9, 17], 20),
        (TextEncoding::Unicode, [0, 3, 11, 14, 22], 25),
    ] {
        let mut config = timing(encoding);
        let result = schedule("A.B。C", &config, 0);
        assert_eq!(result.deadlines, deadlines);
        assert_eq!(result.accumulated_units, units);

        config.skip_spaces = true;
        let result = schedule("A B　C", &config, 0);
        assert_eq!(result.deadlines, [0, 3, 3, 6, 6]);
        assert_eq!(result.accumulated_units, 9);
    }
}

#[test]
fn ruby_inherits_base_times_and_controls_preserve_a_following_automatic_break() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let (markup, layout) = prepare(r"\@[CD/xy]\n\x1;C", encoding, 29);
        let mut config = timing(encoding);
        config.time_scale = Some(1000);
        let result = markup.schedule(&layout, &config, 0).unwrap();
        assert_eq!(layout.entries[7].kind, AtomKind::NewLine);
        assert_eq!(layout.entries[7].atom, None);
        assert_eq!(layout.entries[8].kind, AtomKind::Character('C'));
        assert_eq!(result.deadlines, [0, 0, 300, 300, 300, 600, 600, 0, 600]);
        assert_eq!(result.accumulated_units, 9);

        let result = schedule(r"A[BC/x\wf7;y]D", &config, 0);
        assert_eq!(result.deadlines, [0, 300, 600, 600, 600, 1600]);
        assert_eq!(result.accumulated_units, 19);
    }
}

#[test]
fn missing_context_and_invalid_layout_bindings_have_explicit_results() {
    let mut config = timing(TextEncoding::Cp932);
    config.time_scale = None;
    let result = schedule(r"A\wf7;B", &config, 55);
    assert_eq!(result.deadlines, [0, 0]);
    assert_eq!(result.accumulated_units, 62);
    let (markup, layout) = prepare(r"A\w1;B", config.encoding, 1000);
    assert_eq!(markup.schedule(&layout, &config, 0).unwrap_err().offset, 1);

    let (markup, mut layout) = prepare("AB", config.encoding, 1000);
    layout.entries[0].atom = Some(2);
    assert!(markup.schedule(&layout, &config, 0).is_err());
    layout.entries[0].atom = Some(0);
    layout.entries[0].display_anchor = Some(1);
    assert!(markup.schedule(&layout, &config, 0).is_err());
}
