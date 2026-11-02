use katze_system2::message::{
    AtomKind, BreakCharacters, Layout, LayoutSettings, Markup, StyleDefaults, TextEncoding,
    VerticalAlignment,
};
use std::collections::BTreeMap;

fn settings(width: i32, encoding: TextEncoding) -> LayoutSettings {
    LayoutSettings {
        encoding,
        width,
        margins: [1, 3, 2],
        offset: [0; 2],
        character_spacing: 1,
        line_spacing: 2,
        normal_size: 20,
        ruby_offset: -1,
        vertical_alignment: VerticalAlignment::Preserve,
        line_breaking: true,
        word_wrap: false,
        hanging_indent: false,
        break_characters: BreakCharacters::for_encoding(encoding),
    }
}

fn layout(text: &str, settings: &LayoutSettings) -> Layout {
    let defaults = StyleDefaults {
        text_sizes: [12, 16, 20, 24, 28],
        ruby_sizes: [6, 10, 12, 18, 22],
        text_color: 0xff00_0000,
        ruby_color: 0xff00_0000,
        shadow_color: 0,
    };
    Markup::parse(text, &defaults)
        .unwrap()
        .layout(settings, |ch, size| Ok(size as i32 / 2 + ch as i32 % 3))
        .unwrap()
}

fn lines(layout: &Layout) -> Vec<String> {
    let mut lines = BTreeMap::<u32, String>::new();
    for entry in &layout.entries {
        if let AtomKind::Character(c) = entry.kind {
            if !entry.group.is_some_and(|group| group.ruby) {
                lines.entry(entry.line).or_default().push(c);
            }
        }
    }
    lines.into_values().collect()
}

#[test]
fn punctuation_moves_openers_forward_and_keeps_closers_with_preceding_text() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        assert_eq!(
            lines(&layout("AB(CD", &settings(49, encoding))),
            ["AB", "(CD"]
        );
        assert_eq!(
            lines(&layout("AB)CD", &settings(36, encoding))),
            ["A", "B)", "CD"]
        );
        let mut disabled = settings(36, encoding);
        disabled.line_breaking = false;
        assert_eq!(lines(&layout("AB)CD", &disabled)), ["AB", ")C", "D"]);
    }
}

#[test]
fn groups_move_with_ruby_but_oversized_groups_still_advance() {
    let config = settings(49, TextEncoding::Cp932);
    let result = layout("AB[CD/xy]E", &config);
    assert_eq!(lines(&result), ["AB", "CDE"]);
    assert_eq!(result.entries[2].atom, None);
    assert_eq!(result.entries[5].position, [4, 12]);
    assert_eq!(result.entries[6].position, [13, 12]);
    assert_eq!(result.entries[5].display_anchor, Some(4));
    assert_eq!(result.entries[6].display_anchor, Some(4));
    assert_eq!(result.cursor, Some([36, 25]));
    assert_eq!(result.bottom, 45);

    assert_eq!(
        lines(&layout("A……BC", &settings(36, TextEncoding::Cp932))),
        ["A", "……", "BC"]
    );

    assert_eq!(
        lines(&layout("[ABCDE]F", &settings(25, TextEncoding::Cp932))),
        ["A", "B", "C", "D", "EF"]
    );
}

#[test]
fn hanging_indent_tracks_the_opening_width_and_resumes_after_a_new_opener() {
    let mut config = settings(49, TextEncoding::Cp932);
    config.line_breaking = false;
    config.hanging_indent = true;
    let result = layout("（ABCDEF", &config);
    assert_eq!(lines(&result), ["（AB", "CD", "EF"]);
    assert_eq!(result.entries[4].position, [14, 25]);
    assert_eq!(result.entries[7].position, [14, 47]);

    let result = layout(r"(AB\n(CDEF", &config);
    assert_eq!(lines(&result), ["(AB", "(CD", "EF"]);
    assert_eq!(result.entries[4].position, [1, 25]);
    assert_eq!(result.entries[8].position, [13, 47]);
}

#[test]
fn unicode_word_wrap_and_custom_character_sets_keep_their_distinct_paths() {
    for (encoding, expected, customized) in [
        (
            TextEncoding::Cp932,
            vec!["A ", "C ", "B ", "C ", "B A", "BC"],
            vec!["A", "B C", "D E", "F"],
        ),
        (
            TextEncoding::Unicode,
            vec!["A C ", "B C ", "B ", "ABC"],
            vec!["A", "B ", "CD ", "EF"],
        ),
    ] {
        let mut config = settings(48, encoding);
        config.margins[2] = 3;
        config.hanging_indent = true;
        config.word_wrap = true;
        assert_eq!(lines(&layout("A C B C B ABC", &config)), expected);
        let characters = &mut config.break_characters;
        characters.no_start.push('C');
        characters.no_end.push('A');
        characters.repeated.push('B');
        characters.indent_open.push('B');
        characters.indent_keep.push('C');
        assert_eq!(lines(&layout("AB CD EF", &config)), customized);
    }

    let cp932 = layout("AB CD", &settings(36, TextEncoding::Cp932));
    let unicode = layout("AB CD", &settings(36, TextEncoding::Unicode));
    assert_eq!(lines(&cp932), ["A", "B ", "CD"]);
    assert_eq!(lines(&unicode), ["AB ", "CD"]);

    for (encoding, expected) in [
        (TextEncoding::Cp932, [26, 3]),
        (TextEncoding::Unicode, [1, 25]),
    ] {
        let mut config = settings(49, encoding);
        config.line_breaking = false;
        let result = layout("AB\u{8}CD", &config);
        assert_eq!(result.entries[2].kind, AtomKind::Character('\u{8}'));
        assert_eq!(result.entries[3].position, expected);
    }
}

#[test]
fn ruby_starting_at_negative_x_keeps_scanning_for_its_horizontal_origin() {
    let mut config = settings(49, TextEncoding::Cp932);
    config.margins[0] = -2;
    config.character_spacing = -3;
    let result = layout("[CD/xy]", &config);
    assert_eq!(result.entries[0].position, [-2, 3]);
    assert_eq!(result.entries[1].position, [6, 3]);
    assert_eq!(result.entries[2].position, [9, -10]);
    assert_eq!(result.entries[3].position, [18, -10]);
    assert_eq!(result.entries[2].display_anchor, Some(1));
    assert_eq!(result.entries[3].display_anchor, Some(1));
}
