use katze_system2::message::{
    Alignment, AtomKind, Markup, StyleDefaults, TextGroup, Wait, message_color_argb,
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

fn characters(markup: &Markup) -> String {
    markup
        .atoms
        .iter()
        .filter_map(|atom| match atom.kind {
            AtomKind::Character(c) => Some(c),
            _ => None,
        })
        .collect()
}

#[test]
fn styles_preserve_recovery_colors_relative_sizes_and_preset_terminators() {
    let config = defaults();
    let markup = Markup::parse(
        r"\cd0x80402010;\c;A\f25;\f+3;\f-5;B\fr9;C\fssD\flE\fllF\fn;G\c1;H\c;I",
        &config,
    )
    .unwrap();
    assert_eq!(characters(&markup), "ABCDEF;GHI");
    assert_eq!(
        markup
            .atoms
            .iter()
            .map(|atom| [atom.style.text_size, atom.style.ruby_size])
            .collect::<Vec<_>>(),
        [
            [20, 12],
            [23, 12],
            [23, 9],
            [12, 6],
            [24, 18],
            [28, 22],
            [20, 12],
            [20, 12],
            [20, 12],
            [20, 12],
        ]
    );
    assert_eq!(markup.atoms[8].style.text_color, 1);
    assert_eq!(markup.final_style.text_color, 0x8040_2010);
    assert_eq!(markup.restore_text_color, 0x8040_2010);
    assert_eq!(
        message_color_argb(markup.final_style.text_color),
        0x8010_2040
    );

    let reset = Markup::parse(r"\cr7;A\cr;b\cs9;B\cs;C", &config).unwrap();
    assert_eq!(characters(&reset), "AB;C");
    assert_eq!(reset.atoms[0].style.ruby_color, 7);
    assert_eq!(reset.atoms[1].style.ruby_color, config.ruby_color);
    assert_eq!(reset.atoms[1].style.shadow_color, 9);
    assert_eq!(reset.final_style.shadow_color, config.shadow_color);

    for encoding in [
        katze_system2::message::TextEncoding::Cp932,
        katze_system2::message::TextEncoding::Unicode,
    ] {
        let styled =
            Markup::parse_with_encoding(r"\tBi;A\tbB\tiC\t;D\tib", &config, encoding).unwrap();
        assert_eq!(characters(&styled), "ACDb");
        assert_eq!(
            styled
                .atoms
                .iter()
                .map(|atom| (atom.style.bold, atom.style.italic))
                .collect::<Vec<_>>(),
            [(true, true), (false, true), (false, false), (true, true)]
        );
        assert_eq!(Markup::parse(r"\t", &config).unwrap().atoms[0].source, 1..2);
    }
}

#[test]
fn groups_bind_ruby_and_keep_unicode_byte_positions_without_recursive_brackets() {
    let source = "猫[漢字/かんじ]犬\\[\\q\\\\";
    let markup = Markup::parse(source, &defaults()).unwrap();
    assert_eq!(characters(&markup), "猫漢字かんじ犬[q\\");
    assert_eq!(markup.atoms[0].source, 0..3);
    assert_eq!(markup.atoms[1].source, 4..7);
    assert_eq!(markup.atoms[3].source, 11..14);
    assert_eq!(markup.atoms[7].source, 24..26);
    assert_eq!(markup.atoms[0].group, None);
    for atom in &markup.atoms[1..6] {
        assert_eq!(
            atom.group,
            Some(TextGroup {
                index: 0,
                ruby: atom.source.start >= 11,
            })
        );
    }
    let flat = Markup::parse("[a[b/c]d/e][f", &defaults()).unwrap();
    assert_eq!(characters(&flat), "a[bcd/e]f");
    assert_eq!(flat.atoms[1].group, flat.atoms[0].group);
    assert_eq!(
        flat.atoms[3].group,
        Some(TextGroup {
            index: 0,
            ruby: true
        })
    );
    assert_eq!(flat.atoms[4].group, None);
    assert_eq!(
        flat.atoms[8].group,
        Some(TextGroup {
            index: 1,
            ruby: false
        })
    );
}

#[test]
fn numbers_wrap_and_waits_remain_ordered_operations_with_precise_diagnostics() {
    let markup = Markup::parse(
        r"\c4294967297;A\f0x;B\f-4294967295;C\wf0x10;\w3;\wr\x0x20;\y4;\pc;\@\n",
        &defaults(),
    )
    .unwrap();
    assert_eq!(characters(&markup), "ABC;");
    assert_eq!(markup.atoms[0].style.text_color, 1);
    assert_eq!(markup.atoms[1].style.text_size, 0);
    assert_eq!(markup.atoms[2].style.text_size, 1);
    assert_eq!(
        markup.atoms[3..]
            .iter()
            .map(|atom| atom.kind)
            .collect::<Vec<_>>(),
        [
            AtomKind::Wait(Wait::Frames(16)),
            AtomKind::Wait(Wait::Characters(3)),
            AtomKind::Wait(Wait::Reset),
            AtomKind::X(32),
            AtomKind::Y(4),
            AtomKind::Align(Alignment::Center),
            AtomKind::Character(';'),
            AtomKind::Continue,
            AtomKind::NewLine,
        ]
    );
    assert_eq!(
        Markup::parse("猫\\z42;", &defaults()).unwrap_err().offset,
        3
    );
    assert_eq!(Markup::parse("a\0b", &defaults()).unwrap_err().offset, 1);
}
