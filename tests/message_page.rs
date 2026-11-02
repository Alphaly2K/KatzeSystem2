use katze_system2::{
    Error,
    message::{
        AtomKind, BreakCharacters, LayoutSettings, Page, StyleDefaults, TextEncoding,
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
    page.append(r"\i").unwrap();
    assert_eq!(
        page.prepare(&layout, &timing, |_, _| Ok(10))
            .unwrap_err()
            .offset,
        before.len()
    );
}
