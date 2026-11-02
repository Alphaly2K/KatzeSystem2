use katze_system2::message::{
    AtomKind, LayoutSettings, Markup, StyleDefaults, TextEncoding, TextGroup, VerticalAlignment,
};

fn defaults() -> StyleDefaults {
    StyleDefaults {
        text_sizes: [6, 8, 10, 14, 18],
        ruby_sizes: [2, 3, 4, 6, 8],
        text_color: 0xff00_0000,
        ruby_color: 0xff00_0000,
        shadow_color: 0,
    }
}

fn settings() -> LayoutSettings {
    LayoutSettings {
        encoding: TextEncoding::Cp932,
        width: 12,
        margins: [1, 3, 1],
        offset: [0; 2],
        character_spacing: 1,
        line_spacing: 2,
        normal_size: 10,
        ruby_offset: 1,
        vertical_alignment: VerticalAlignment::Preserve,
        line_breaking: false,
        word_wrap: false,
        hanging_indent: false,
        break_characters: Default::default(),
    }
}

#[test]
fn wrapped_fullwidth_space_keeps_distinct_cp932_and_unicode_layouts() {
    let markup = Markup::parse("AB　CD", &defaults()).unwrap();
    let cp932 = markup.layout(&settings(), |_, _| Ok(4)).unwrap();
    let unicode = markup
        .layout(
            &LayoutSettings {
                encoding: TextEncoding::Unicode,
                ..settings()
            },
            |_, _| Ok(4),
        )
        .unwrap();
    assert_eq!(cp932.entries.len(), 6);
    assert_eq!(unicode.entries.len(), 7);
    assert_eq!(cp932.entries[3].size[0], 0);
    assert_eq!(unicode.entries[3].size[0], 4);
    assert_eq!(cp932.entries.last().unwrap().position, [7, 15]);
    assert_eq!(unicode.entries.last().unwrap().position, [1, 27]);
}

#[test]
fn automatic_breaks_keep_source_indices_and_separate_cursor_from_generated_records() {
    let markup = Markup::parse("ABCD", &defaults()).unwrap();
    let layout = markup.layout(&settings(), |_, _| Ok(4)).unwrap();
    assert_eq!(
        layout.entries.iter().map(|e| e.atom).collect::<Vec<_>>(),
        [Some(0), Some(1), None, Some(2), Some(3)]
    );
    assert_eq!(
        layout
            .entries
            .iter()
            .map(|e| (e.position, e.line))
            .collect::<Vec<_>>(),
        [
            ([1, 3], 0),
            ([6, 3], 0),
            ([0, 0], 0),
            ([1, 15], 1),
            ([6, 15], 1)
        ]
    );
    assert_eq!(layout.entries[2].kind, AtomKind::NewLine);
    assert_eq!(layout.entries[2].size, [0; 2]);
    assert_eq!(layout.cursor, Some([10, 15]));
    assert_eq!(layout.bottom, 25);
    let oversized = Markup::parse("A", &defaults())
        .unwrap()
        .layout(
            &LayoutSettings {
                width: 0,
                ..settings()
            },
            |_, _| Ok(40),
        )
        .unwrap();
    assert_eq!(oversized.cursor, Some([41, 3]));
    assert_eq!(oversized.entries[1].kind, AtomKind::NewLine);
    assert_eq!(oversized.entries[1].atom, None);
    assert_eq!(oversized.entries[1].position, [1, 15]);
    assert_eq!(oversized.entries[1].line, 1);
}

#[test]
fn mixed_sizes_align_per_line_without_rewriting_newline_or_cursor_geometry() {
    let markup = Markup::parse(r"\f6;A\f14;B\nC", &defaults()).unwrap();
    for (alignment, first_y) in [
        (VerticalAlignment::Preserve, 3),
        (VerticalAlignment::Top, 3),
        (VerticalAlignment::Bottom, 11),
    ] {
        let layout = markup
            .layout(
                &LayoutSettings {
                    width: 36,
                    vertical_alignment: alignment,
                    ..settings()
                },
                |_, size| Ok(size as i32 / 2),
            )
            .unwrap();
        assert_eq!(layout.entries[0].position, [1, first_y]);
        assert_eq!(layout.entries[1].position, [5, 3]);
        assert_eq!(layout.entries[2].position, [13, 3]);
        assert_eq!(layout.entries[3].position, [1, 19]);
        assert_eq!(layout.cursor, Some([8, 19]));
        assert_eq!(layout.bottom, 33);
    }
}

#[test]
fn ruby_distributes_integer_gaps_and_binds_display_to_the_selected_base_glyph() {
    let markup = Markup::parse("[AB/xyz]C", &defaults()).unwrap();
    let layout = markup
        .layout(
            &LayoutSettings {
                width: 40,
                ..settings()
            },
            |_, size| Ok(if size == 4 { 2 } else { 4 }),
        )
        .unwrap();
    assert_eq!(
        layout.entries[2..5]
            .iter()
            .map(|e| (e.position, e.display_anchor))
            .collect::<Vec<_>>(),
        [([1, 0], Some(0)), ([3, 0], Some(1)), ([6, 0], Some(1))]
    );
    assert!(layout.entries[2..5].iter().all(|e| e.line == 0));
    assert_eq!(layout.entries[5].position, [11, 3]);
    assert_eq!(layout.cursor, Some([15, 3]));
    assert_eq!(layout.bottom, 13);
    let wide = Markup::parse(r"\fr8;[AB/xyz]C", &defaults())
        .unwrap()
        .layout(&settings(), |_, size| Ok(if size == 8 { 6 } else { 4 }))
        .unwrap();
    assert_eq!(
        wide.entries[2..5]
            .iter()
            .map(|e| (e.position, e.display_anchor))
            .collect::<Vec<_>>(),
        [([-6, -4], Some(0)), ([0, -4], Some(0)), ([6, -4], Some(1))]
    );
}

#[test]
fn position_tags_keep_the_original_record_categories_and_group_counter() {
    let markup = Markup::parse(r"A\x5;B\y9;C", &defaults()).unwrap();
    let layout = markup
        .layout(
            &LayoutSettings {
                width: 40,
                ..settings()
            },
            |_, _| Ok(4),
        )
        .unwrap();
    assert_eq!(
        layout
            .entries
            .iter()
            .map(|e| e.position)
            .collect::<Vec<_>>(),
        [[1, 0], [0, 0], [0, 0], [9, 0], [5, 0]]
    );
    assert_eq!(layout.entries[1].size, [0; 2]);
    assert_eq!(layout.cursor, Some([9, 0]));
    let grouped = Markup::parse(r"[]\x1;A", &defaults()).unwrap();
    assert_eq!(
        grouped.atoms[0].group,
        Some(TextGroup {
            index: 1,
            ruby: false
        })
    );
    assert!(
        Markup::parse(r"\x2;A", &defaults())
            .unwrap()
            .layout(&settings(), |_, _| Ok(4))
            .is_err()
    );
    let split = Markup::parse(r"[AB/cd]\x2;E", &defaults())
        .unwrap()
        .layout(
            &LayoutSettings {
                width: 10,
                ..settings()
            },
            |_, _| Ok(4),
        )
        .unwrap();
    assert!(
        split
            .entries
            .iter()
            .filter(|e| e.group.is_some_and(|g| g.ruby))
            .all(|e| e.display_anchor.is_none())
    );
}
