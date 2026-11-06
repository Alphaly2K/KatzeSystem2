use katze_system2::{
    Error,
    message::{
        AtomKind, BreakCharacters, LayoutSettings, Markup, Page, StyleDefaults, TextEncoding,
        TimingSettings, VerticalAlignment,
    },
};

fn defaults() -> StyleDefaults {
    StyleDefaults {
        text_sizes: [12, 16, 20, 24, 28],
        ruby_sizes: [6, 10, 12, 18, 22],
        text_color: 0xff12_3456,
        ruby_color: 0xff77_8899,
        shadow_color: 0xff00_1122,
    }
}

fn settings(encoding: TextEncoding) -> (LayoutSettings, TimingSettings) {
    (
        LayoutSettings {
            encoding,
            width: 1000,
            margins: [0; 3],
            offset: [0; 2],
            character_spacing: 0,
            line_spacing: 0,
            normal_size: 20,
            ruby_offset: 0,
            vertical_alignment: VerticalAlignment::Preserve,
            line_breaking: false,
            word_wrap: false,
            hanging_indent: false,
            break_characters: BreakCharacters::for_encoding(encoding),
        },
        TimingSettings {
            encoding,
            time_scale: Some(10),
            character_delay: 3,
            period_delay: 8,
            skip_spaces: false,
        },
    )
}

#[test]
fn deferred_append_keeps_styles_and_groups_until_clear() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let (layout, timing) = settings(encoding);
        let mut page = Page::new(defaults(), encoding);
        page.append(r"\f+3;\c7;[AB/").unwrap();
        page.append("xy]C").unwrap();
        assert!(page.markup().atoms.is_empty());
        assert!(page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap());
        assert_eq!(page.markup().atoms.len(), 5);
        assert_eq!(page.markup().final_style.text_size, 23);
        assert_eq!(page.markup().final_style.text_color, 7);
        assert_eq!(page.schedule().accumulated_units, 9);
        assert!(!page.prepare(&layout, &timing, |_, _| panic!()).unwrap());

        page.append("[D/z]").unwrap();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        let atoms = &page.markup().atoms;
        assert_eq!(atoms[0].group.unwrap().index, 0);
        assert_eq!(atoms[5].group.unwrap().index, 1);
        assert_eq!(atoms[5].style.text_size, 23);
        assert_eq!(page.schedule().accumulated_units, 12);
        assert_eq!(&page.source()[atoms[5].source.clone()], "D");

        page.clear();
        assert!(page.source().is_empty());
        assert!(page.layout().entries.is_empty());
        assert_eq!(page.markup().final_style, defaults().style());
        assert_eq!(page.schedule().accumulated_units, 0);
        page.append("[E/u]").unwrap();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        assert_eq!(page.markup().atoms[0].group.unwrap().index, 0);
        assert_eq!(page.schedule().deadlines, [0, 0]);
    }
}

#[test]
fn identifiers_keep_first_values_wait_for_prepare_and_clear_with_the_page() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let (layout, timing) = settings(encoding);
        let mut page = Page::new(defaults(), encoding);
        page.append(r"A\id1:4;\ID1:9;B\id0xffffffff:4294967297;")
            .unwrap();
        assert_eq!(page.identifier(1), 0);
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        assert_eq!((page.identifier(1), page.identifier(-1)), (4, 1));
        assert_eq!(page.schedule().deadlines, [0, 3]);
        page.append(r"\id2:8;C").unwrap();
        assert!(
            page.prepare(&layout, &timing, |_, _| Err(Error::new(0, "度量失败")))
                .is_err()
        );
        assert_eq!((page.identifier(1), page.identifier(2)), (4, 0));
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        assert_eq!(page.identifier(2), 8);
        page.append(r"\id3").unwrap();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        let characters: String = page
            .markup()
            .atoms
            .iter()
            .map(|atom| match atom.kind {
                AtomKind::Character(c) => c,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(characters, "ABC3");
        assert_eq!(page.markup().identifiers.get(&3), Some(&0));
        page.invalidate_layout();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        assert_eq!(page.identifier(1), 4);
        page.clear();
        assert!(page.markup().identifiers.is_empty());
        page.append(r"\id:9;D\id0x:7;E\id7;").unwrap();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        assert_eq!(page.identifier(0), 0);
        assert_eq!(
            page.markup().atoms.last().unwrap().kind,
            AtomKind::Character(';')
        );
        assert!(page.markup().identifiers.contains_key(&7));
    }
}

#[test]
fn control_escapes_keep_encoding_specific_records_groups_and_timing() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let (layout, timing) = settings(encoding);
        let mut page = Page::new(defaults(), encoding);
        page.append(r"A\iB\rC\sD\u42;").unwrap();
        page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        let special = if encoding == TextEncoding::Cp932 {
            '\u{3000}'
        } else {
            '\u{8140}'
        };
        let kinds: Vec<_> = page.markup().atoms.iter().map(|atom| atom.kind).collect();
        assert_eq!(
            kinds,
            [
                AtomKind::Character('A'),
                AtomKind::Character('\u{6}'),
                AtomKind::Character('B'),
                AtomKind::Character('\u{8}'),
                AtomKind::Character('C'),
                AtomKind::Character(special),
                AtomKind::NewLine,
                AtomKind::Character('D'),
                AtomKind::Character(if encoding == TextEncoding::Cp932 {
                    'u'
                } else {
                    '\u{6}'
                }),
                AtomKind::Character('4'),
                AtomKind::Character('2'),
                AtomKind::Character(';'),
            ]
        );
        assert_eq!(page.markup().atoms[5].source, 7..9);
        assert_eq!(page.markup().atoms[6].source, 7..9);
        assert_eq!(page.layout().cursor, Some([10, 0]));
        assert_eq!(page.layout().entries[1].position, [0, 20]);
        assert_eq!(page.layout().entries[2].position, [0; 2]);
        assert_eq!(
            page.schedule().deadlines,
            if encoding == TextEncoding::Cp932 {
                [0, 3, 3, 6, 6, 9, 12, 12, 15, 18, 21, 24]
            } else {
                [0, 3, 3, 6, 6, 9, 12, 12, 15, 15, 18, 21]
            }
        );
        assert_eq!(
            page.remaining(u32::MAX).unwrap(),
            if encoding == TextEncoding::Cp932 {
                9
            } else {
                8
            }
        );
        assert!(page.advance(u32::MAX, u32::MAX).unwrap().complete);
        assert_eq!(page.remaining(u32::MAX).unwrap(), 0);

        let mut breaking = layout.clone();
        breaking.line_breaking = true;
        for text in [r"A\iB", r"\pcA\iB", r"\prA\iB"] {
            let marked = Markup::parse_with_encoding(text, &defaults(), encoding).unwrap();
            let positions = marked.layout(&breaking, |_, _| Ok(10)).unwrap();
            assert_eq!(positions.cursor, Some([10, 20]));
            assert_eq!(positions.entries.last().unwrap().position, [0, 20]);
        }

        let grouped =
            Markup::parse_with_encoding(r"[A/\iB\rC\sD]\U", &defaults(), encoding).unwrap();
        for index in [1, 3, 5, 6] {
            assert!(grouped.atoms[index].group.is_none());
        }
        for index in [2, 4, 7] {
            assert!(grouped.atoms[index].group.unwrap().ruby);
        }
        assert_eq!(
            grouped.atoms.last().unwrap().kind,
            AtomKind::Character(if encoding == TextEncoding::Cp932 {
                'U'
            } else {
                '\u{6}'
            })
        );
    }
}

#[test]
fn prepared_unclosed_group_ends_before_later_append() {
    let encoding = TextEncoding::Cp932;
    let (layout, timing) = settings(encoding);
    let mut page = Page::new(defaults(), encoding);
    page.append("[A/").unwrap();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    page.append("x]B[C/y]").unwrap();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    let atoms = &page.markup().atoms;
    assert_eq!(atoms[0].group.unwrap().index, 0);
    assert_eq!(atoms[1].kind, AtomKind::Character('x'));
    assert_eq!(atoms[2].kind, AtomKind::Character(']'));
    assert!(atoms[1..4].iter().all(|atom| atom.group.is_none()));
    assert_eq!(atoms[4].group.unwrap().index, 1);
}

#[test]
fn old_breaks_and_deadlines_survive_wider_layout_and_new_timing() {
    let encoding = TextEncoding::Cp932;
    let (mut layout, mut timing) = settings(encoding);
    layout.width = 25;
    let mut page = Page::new(defaults(), encoding);
    page.append("ABCD").unwrap();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    assert_eq!(page.schedule().deadlines, [0, 3, 0, 6, 9]);

    layout.width = 100;
    timing.time_scale = Some(100);
    timing.character_delay = 7;
    page.append("E").unwrap();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    assert_eq!(page.schedule().deadlines, [0, 3, 0, 6, 9, 120]);
    assert_eq!(page.schedule().accumulated_units, 19);
    assert_eq!(page.layout().entries[2].kind, AtomKind::NewLine);
    assert_eq!(page.layout().cursor, Some([30, 20]));

    page.invalidate_layout();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    assert_eq!(page.schedule().deadlines, [0, 3, 0, 6, 9, 120]);
    assert_eq!(page.schedule().accumulated_units, 19);
}

#[test]
fn ruby_without_a_new_binding_retains_its_previous_deadline() {
    let encoding = TextEncoding::Unicode;
    let (mut layout, mut timing) = settings(encoding);
    layout.width = 55;
    layout.margins = [1, 3, 2];
    layout.character_spacing = 2;
    layout.line_spacing = 2;
    layout.ruby_offset = -1;
    layout.line_breaking = true;
    layout.word_wrap = true;
    timing.time_scale = Some(1000);
    timing.skip_spaces = true;
    let mut page = Page::new(defaults(), encoding);
    page.append(r"\f+3;\c0x112233;A").unwrap();
    page.append("[AB/xy]").unwrap();
    page.prepare(&layout, &timing, |c, size| {
        Ok(size as i32 / 2 + c as i32 % 3)
    })
    .unwrap();
    page.append("C。").unwrap();
    page.prepare(&layout, &timing, |c, size| {
        Ok(size as i32 / 2 + c as i32 % 3)
    })
    .unwrap();
    let ruby: Vec<_> = page
        .layout()
        .entries
        .iter()
        .zip(&page.schedule().deadlines)
        .filter(|(entry, _)| entry.group.is_some_and(|group| group.ruby))
        .collect();
    assert_eq!(ruby.len(), 2);
    assert!(ruby.iter().all(|(entry, _)| entry.display_anchor.is_none()));
    assert!(ruby.iter().all(|(_, deadline)| **deadline == 600));
}

#[test]
fn prepare_failure_keeps_previous_page_and_global_source_offsets() {
    let encoding = TextEncoding::Unicode;
    let (layout, timing) = settings(encoding);
    let mut page = Page::new(defaults(), encoding);
    page.append("あ").unwrap();
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    let old_markup = page.markup().clone();
    let old_layout = page.layout().clone();
    let old_schedule = page.schedule().clone();
    page.append(r"\f+3;B").unwrap();
    assert!(
        page.prepare(&layout, &timing, |_, _| Err(Error::new(0, "度量失败")))
            .is_err()
    );
    assert_eq!(page.markup(), &old_markup);
    assert_eq!(page.layout(), &old_layout);
    assert_eq!(page.schedule(), &old_schedule);
    assert!(page.is_dirty());
    page.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
    assert_eq!(page.markup().atoms[1].style.text_size, 23);
    assert_eq!(page.markup().atoms[1].source, 8..9);
    assert_eq!(page.schedule().accumulated_units, 6);

    let before = page.source().to_owned();
    assert_eq!(page.append("C\0").unwrap_err().offset, before.len() + 1);
    assert_eq!(page.source(), before);
    page.append(r"\z").unwrap();
    assert_eq!(
        page.prepare(&layout, &timing, |_, _| Ok(10))
            .unwrap_err()
            .offset,
        before.len()
    );
}
