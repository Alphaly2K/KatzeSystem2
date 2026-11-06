use katze_system2::{
    graphics::Surface,
    message::{
        BreakCharacters, Font, FontRequest, GlyphEffects, GlyphMask, LayoutSettings, Page,
        StyleDefaults, TextEncoding, TimingSettings, VerticalAlignment,
    },
};

fn font() -> Font {
    Font::from_bytes(include_bytes!("fixtures/synthetic-triangle.ttf"), 0).unwrap()
}

#[test]
fn fixed_pitch_uses_cp932_mapping_and_preserves_wide_platform_advances() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let mut request = FontRequest {
            encoding,
            name: Vec::new(),
            weight: 400,
            italic: false,
            backend: 0,
            glyph_parameter: 1,
            measure_parameter: 0,
        };
        for (character, size, measured, fixed, expected) in [
            ('あ', 21, 6, 1, 21),
            ('Ж', 21, 25, 1, 21),
            ('ｱ', 21, 5, 1, 11),
            ('é', 21, 0, 1, 11),
            ('A', 20, 4, 1, 10),
            ('A', 21, 25, 1, 25),
            ('A', 20, -1, u32::MAX, 10),
            ('あ', 20, 7, 0, 7),
            ('A', 21, 7, 0, 7),
            ('一', 0, 25, 1, 0),
        ] {
            request.measure_parameter = fixed;
            assert_eq!(
                request
                    .character_advance(character, size, measured)
                    .unwrap(),
                expected
            );
        }
        assert!(request.character_advance('A', u32::MAX, 10).is_err());
    }
}

#[test]
fn explicit_font_measures_and_rasterizes_a_fixed_outline_without_system_fonts() {
    let font = font();
    assert!(font.has_character('A'));
    assert!(font.has_character(' '));
    assert!(!font.has_character('B'));
    assert_eq!(font.measure('A', 20).unwrap(), 20);
    assert_eq!(font.measure(' ', 20).unwrap(), 10);
    let glyph = font.rasterize('A', 20).unwrap();
    assert_eq!(glyph.size(), [16, 16]);
    assert_eq!(glyph.offset(), [3, 1]);
    assert_eq!(glyph.advance(), 20);
    assert!(glyph.coverage().contains(&64));
    assert!(glyph.coverage().iter().any(|&v| v > 0 && v < 64));
    let space = font.rasterize(' ', 20).unwrap();
    assert_eq!(space.advance(), 10);
    assert!(space.coverage().is_empty());
    assert!(font.rasterize('A', 0).unwrap().coverage().is_empty());
    assert!(font.rasterize('A', u32::MAX).is_err());
    assert!(font.rasterize('A', 1_000_000).is_err());
    assert!(
        font.measure('B', 20)
            .unwrap_err()
            .message
            .contains("U+0042")
    );
    assert!(Font::from_bytes(b"invalid", 0).is_err());
    assert!(Font::from_bytes(include_bytes!("fixtures/synthetic-triangle.ttf"), 1).is_err());
}

#[test]
fn gray_lookup_preserves_rgb_and_uses_coverage_instead_of_color_alpha() {
    let mask = GlyphMask::new([5, 1], [0; 2], 5, vec![0, 1, 32, 63, 64]).unwrap();
    for (double, alpha) in [
        (false, [0, 3, 127, 251, 255]),
        (true, [0, 6, 254, 255, 255]),
    ] {
        let image = mask.colorize(0x1103_0201, double).unwrap();
        for (x, a) in alpha.into_iter().enumerate() {
            assert_eq!(image.pixel(x as u32, 0), Some((a << 24) | 0x0001_0203));
        }
        assert_eq!(mask.colorize(0xff03_0201, double).unwrap(), image);
    }
    assert!(GlyphMask::new([1, 1], [0; 2], 0, vec![65]).is_err());
    assert!(GlyphMask::new([2, 1], [0; 2], 0, vec![0]).is_err());
    assert!(GlyphMask::new([u32::MAX, 0], [0; 2], 0, vec![]).is_err());
}

#[test]
fn prepared_page_paints_visible_text_and_ruby_with_their_respective_styles() {
    let font = font();
    let mut page = Page::new(
        StyleDefaults {
            text_sizes: [20; 5],
            ruby_sizes: [12; 5],
            text_color: 0x0011_2233,
            ruby_color: 0x0044_5566,
            shadow_color: 0,
        },
        TextEncoding::Unicode,
    );
    page.append(r"A\tb;[A/\ti;A]").unwrap();
    page.prepare(
        &LayoutSettings {
            encoding: TextEncoding::Unicode,
            width: 100,
            margins: [0, 20, 0],
            offset: [0; 2],
            character_spacing: 0,
            line_spacing: 0,
            normal_size: 20,
            ruby_offset: 0,
            vertical_alignment: VerticalAlignment::Preserve,
            line_breaking: false,
            word_wrap: false,
            hanging_indent: false,
            break_characters: BreakCharacters::for_encoding(TextEncoding::Unicode),
        },
        &TimingSettings {
            encoding: TextEncoding::Unicode,
            time_scale: Some(10),
            character_delay: 2,
            period_delay: 5,
            skip_spaces: false,
        },
        |c, size| font.measure(c, size),
    )
    .unwrap();
    let mut target = Surface::new([100, 60]).unwrap();
    let mut requests = Vec::new();
    page.advance(0, u32::MAX).unwrap();
    assert!(
        page.paint_foreground(&mut target, [0; 2], |c, size| {
            requests.push((c, size));
            font.rasterize(c, size)
        })
        .unwrap()
    );
    assert_eq!(requests, [('A', 20)]);
    assert!(target.pixels().chunks_exact(4).any(|p| p[3] == 255));

    requests.clear();
    target.clear(0);
    page.advance(2, u32::MAX).unwrap();
    page.paint_foreground(&mut target, [0; 2], |c, size| {
        requests.push((c, size));
        font.rasterize(c, size)
    })
    .unwrap();
    assert_eq!(requests, [('A', 20), ('A', 20), ('A', 12)]);
    assert!(
        target
            .pixels()
            .chunks_exact(4)
            .any(|p| p == [0x44, 0x55, 0x66, 255])
    );
    assert!(
        target
            .pixels()
            .chunks_exact(4)
            .any(|p| p == [0x11, 0x22, 0x33, 255])
    );
    target.clear(0);
    requests.clear();
    let mut styles = Vec::new();
    page.paint_styled(
        &mut target,
        [0; 2],
        &GlyphEffects {
            outline: true,
            shadow: true,
            ..Default::default()
        },
        false,
        |c, size, style| {
            requests.push((c, size));
            styles.push((style.bold, style.italic));
            font.rasterize(c, size)
        },
    )
    .unwrap();
    assert_eq!(requests, [('A', 20), ('A', 20), ('A', 12)]);
    assert_eq!(styles, [(false, false), (true, false), (false, true)]);
    assert!(
        target
            .pixels()
            .chunks_exact(4)
            .any(|p| p[..3] == [0; 3] && p[3] > 0)
    );
    target.clear(0);
    assert!(
        !page
            .paint_foreground_styled(&mut target, [-1000; 2], |c, size, _| font
                .rasterize(c, size))
            .unwrap()
    );
    page.append("A").unwrap();
    assert!(
        page.paint_foreground(&mut target, [0; 2], |_, _| unreachable!())
            .is_err()
    );
}
