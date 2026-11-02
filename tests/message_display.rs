use katze_system2::{
    Error, Result,
    graphics::Rect,
    message::{
        BreakCharacters, Display, DisplaySettings, Font, GlyphEffects, GlyphMask, GlyphState,
        LayoutSettings, StyleDefaults, TextEncoding, TimingSettings, VerticalAlignment,
    },
};

fn defaults() -> StyleDefaults {
    StyleDefaults {
        text_sizes: [20; 5],
        ruby_sizes: [12; 5],
        text_color: 0x0011_2233,
        ruby_color: 0x0044_5566,
        shadow_color: 0,
    }
}

fn layout() -> LayoutSettings {
    LayoutSettings {
        encoding: TextEncoding::Unicode,
        width: 80,
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
        break_characters: BreakCharacters::for_encoding(TextEncoding::Unicode),
    }
}

fn timing() -> TimingSettings {
    TimingSettings {
        encoding: TextEncoding::Unicode,
        time_scale: Some(10),
        character_delay: 10,
        period_delay: 30,
        skip_spaces: false,
    }
}

fn mask(_: char, _: u32) -> Result<GlyphMask> {
    GlyphMask::new([3; 2], [1; 2], 10, vec![64; 9])
}

fn display(width: u32, text: &str) -> Display {
    let mut display = Display::new(
        defaults(),
        TextEncoding::Unicode,
        [80, 48],
        DisplaySettings {
            reveal_width: width,
            ..Default::default()
        },
    )
    .unwrap();
    display.append(text).unwrap();
    display
        .prepare(&layout(), &timing(), |_, _| Ok(10))
        .unwrap();
    display
}

#[test]
fn reveal_prepares_each_segment_once_and_keeps_completed_glyphs_on_time_reversal() {
    let mut display = display(4, "A\u{ffff}B");
    let first = display.advance(0, 0, mask).unwrap();
    assert_eq!(first.prepared, [0]);
    assert!(first.completed.is_empty());
    assert!(!first.complete);
    assert_eq!(display.page().glyph_state(0), Some(GlyphState::Revealing));
    assert_eq!(display.page().is_visible(0), Some(false));
    assert_eq!(display.page().remaining(0).unwrap(), 1);
    assert_eq!(display.page().remaining(u32::MAX).unwrap(), 2);
    assert!(
        display
            .surface()
            .pixels()
            .chunks_exact(4)
            .all(|p| p[3] == 0)
    );
    assert!(
        display
            .source_surface()
            .unwrap()
            .pixels()
            .chunks_exact(4)
            .any(|p| p[3] == 255)
    );

    let first = display
        .advance(100, 0, |_, _| panic!("prepared glyph was rasterized again"))
        .unwrap();
    assert!(first.prepared.is_empty());
    assert_eq!(first.completed, [0]);
    assert!(first.complete);
    let first_pixel = display.surface().pixel(1, 1);
    let second = display.advance(0, u32::MAX, mask).unwrap();
    assert_eq!(second.prepared, [2]);
    assert!(second.completed.is_empty());
    assert!(!second.complete);
    assert_eq!(display.surface().pixel(1, 1), first_pixel);
    let second = display.advance(u32::MAX, u32::MAX, mask).unwrap();
    assert_eq!(second.completed, [2]);
    assert!(second.complete);
    let complete = display.surface().clone();
    assert!(
        display
            .advance(0, u32::MAX, |_, _| unreachable!())
            .unwrap()
            .complete
    );
    assert_eq!(display.surface(), &complete);

    display.append("C").unwrap();
    assert!(display.advance(0, u32::MAX, mask).is_err());
    display
        .prepare(&layout(), &timing(), |_, _| Ok(10))
        .unwrap();
    let appended = display.advance(0, u32::MAX, mask).unwrap();
    assert_eq!(appended.prepared, [3]);
    assert!(!appended.complete);
    assert_eq!(display.page().glyph_state(0), Some(GlyphState::Complete));
}

#[test]
fn raster_failure_retries_only_the_failed_glyph_and_preserves_pending_color_sync() {
    for width in [0, 4] {
        let mut display = display(width, "A\u{ffff}B");
        assert!(
            display
                .advance(u32::MAX, u32::MAX, |c, size| {
                    if c == 'B' {
                        Err(Error::new(0, "glyph unavailable"))
                    } else {
                        mask(c, size)
                    }
                })
                .is_err()
        );
        assert_eq!(display.page().glyph_state(2), Some(GlyphState::Hidden));
        assert_eq!(display.dirty_rectangle(), Some(Rect::from_xywh(0, 0, 7, 7)));
        assert_eq!(
            display.page().glyph_state(0),
            Some(if width == 0 {
                GlyphState::Complete
            } else {
                GlyphState::Revealing
            })
        );
        let recovered = display
            .advance(u32::MAX, 0, |_, _| panic!("unexpected rasterization"))
            .unwrap();
        assert!(recovered.complete);
        assert_eq!(display.surface().pixel(1, 1), Some(0xff33_2211));
        assert_eq!(display.page().remaining(u32::MAX).unwrap(), 1);
        let mut requests = Vec::new();
        let last = display
            .advance(u32::MAX, u32::MAX, |c, size| {
                requests.push(c);
                mask(c, size)
            })
            .unwrap();
        assert_eq!(requests, ['B']);
        assert_eq!(last.prepared, [2]);
        assert_eq!(last.completed, [2]);
        assert!(last.complete);
    }
}

#[test]
fn display_paints_font_effects_and_ruby_and_clear_preserves_speed() {
    let font = Font::from_bytes(include_bytes!("fixtures/synthetic-triangle.ttf"), 0).unwrap();
    let mut display = Display::new(
        defaults(),
        TextEncoding::Unicode,
        [80, 64],
        DisplaySettings {
            effects: GlyphEffects {
                outline: true,
                shadow: true,
                ..Default::default()
            },
            reveal_width: 4,
            double_coverage: false,
        },
    )
    .unwrap();
    let mut layout = layout();
    layout.margins[1] = 20;
    display.append("[A/A]").unwrap();
    display
        .prepare(&layout, &timing(), |c, s| font.measure(c, s))
        .unwrap();
    assert!(
        display
            .advance(u32::MAX, u32::MAX, |c, s| font.rasterize(c, s))
            .unwrap()
            .complete
    );
    assert!(
        display
            .surface()
            .pixels()
            .chunks_exact(4)
            .any(|p| p == [0x11, 0x22, 0x33, 255])
    );
    assert!(
        display
            .surface()
            .pixels()
            .chunks_exact(4)
            .any(|p| p == [0x44, 0x55, 0x66, 255])
    );
    display.set_speed(37);
    let page = display.page().clone();
    let dirty = display.dirty_rectangle();
    display.clear_pixels();
    assert_eq!(display.page(), &page);
    assert_eq!(display.dirty_rectangle(), dirty);
    for surface in [display.surface(), display.source_surface().unwrap()] {
        assert!(surface.pixels().iter().all(|&byte| byte == 0));
    }
    display.clear();
    assert!(display.page().is_dirty());
    assert_eq!(display.page().speed(), 37);
    for surface in [display.surface(), display.source_surface().unwrap()] {
        assert!(surface.pixels().chunks_exact(4).all(|p| p[3] == 0));
    }
}

#[test]
fn offscreen_records_complete_without_requesting_glyphs() {
    for width in [0, 4] {
        let mut display = display(width, "A");
        let mut layout = layout();
        layout.offset[0] = 1000;
        display.invalidate_layout();
        display.prepare(&layout, &timing(), |_, _| Ok(10)).unwrap();
        let result = display
            .advance(u32::MAX, u32::MAX, |_, _| {
                panic!("offscreen glyph requested")
            })
            .unwrap();
        assert!(result.complete);
        assert_eq!(result.prepared, [0]);
        assert_eq!(result.completed, [0]);
        assert!(display.surface().pixels().iter().all(|&p| p == 0));
        assert_eq!(display.dirty_rectangle(), None);
    }
}

#[test]
fn ordinary_dirty_uses_clipped_blit_and_survives_clear_until_acknowledged() {
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        let mut display =
            Display::new(defaults(), encoding, [80, 48], DisplaySettings::default()).unwrap();
        display.append("A").unwrap();
        assert_eq!(display.dirty_rectangle(), None);
        let mut layout = layout();
        layout.encoding = encoding;
        let mut timing = timing();
        timing.encoding = encoding;
        layout.offset = [-2, -1];
        display.invalidate_layout();
        display.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        display.advance(u32::MAX, u32::MAX, mask).unwrap();
        let (height, top_pixel) = if encoding == TextEncoding::Cp932 {
            (6, 0)
        } else {
            (7, 0xff33_2211)
        };
        let expected = Some(Rect::from_xywh(0, 0, 5, height));
        assert_eq!(display.surface().pixel(1, 0), Some(top_pixel));
        assert_eq!(display.dirty_rectangle(), expected);
        assert_eq!(display.dirty_rectangle(), expected);
        display.clear();
        assert_eq!(display.dirty_rectangle(), expected);
        display.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        display.advance(u32::MAX, u32::MAX, mask).unwrap();
        assert_eq!(display.dirty_rectangle(), expected);
        display.clear_dirty();
        assert_eq!(display.dirty_rectangle(), None);

        display.append("AB").unwrap();
        display.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        display.advance(u32::MAX, u32::MAX, mask).unwrap();
        assert_eq!(
            display.dirty_rectangle(),
            Some(Rect::from_xywh(0, 0, 15, height))
        );
        display.clear_dirty();
        layout.offset = [4, 2];
        display.invalidate_layout();
        display.prepare(&layout, &timing, |_, _| Ok(10)).unwrap();
        display.advance(u32::MAX, u32::MAX, mask).unwrap();
        assert_eq!(display.dirty_rectangle(), None);
        display.clear();
        assert_eq!(display.dirty_rectangle(), None);
    }
}

#[test]
fn reveal_dirty_includes_prepared_source_and_repeated_active_alpha_updates() {
    let mut display = display(4, "A\u{ffff}B");
    display.advance(0, 0, mask).unwrap();
    let first = Some(Rect::from_xywh(0, 0, 7, 7));
    assert_eq!(display.dirty_rectangle(), first);
    assert!(
        display
            .surface()
            .pixels()
            .chunks_exact(4)
            .all(|p| p[3] == 0)
    );
    display.clear_dirty();
    let repeated = display.advance(0, 0, |_, _| unreachable!()).unwrap();
    assert!(repeated.prepared.is_empty());
    assert_eq!(display.dirty_rectangle(), first);
    display.advance(100, 0, mask).unwrap();
    display.clear_dirty();
    display.advance(100, 0, mask).unwrap();
    assert_eq!(display.dirty_rectangle(), None);

    display.advance(0, u32::MAX, mask).unwrap();
    let second = Some(Rect::from_xywh(10, 0, 7, 7));
    assert_eq!(display.dirty_rectangle(), second);
    display.clear_dirty();
    display.advance(0, u32::MAX, mask).unwrap();
    assert_eq!(display.dirty_rectangle(), None);
    display.advance(3, u32::MAX, mask).unwrap();
    assert_eq!(display.dirty_rectangle(), second);
    display.clear();
    assert_eq!(display.dirty_rectangle(), second);
}

#[test]
fn reconfiguration_replays_source_discards_old_breaks_and_resets_display_time() {
    let mut display = display(0, r"\f+3;AB[C");
    let mut narrow = layout();
    narrow.width = 10;
    display.invalidate_layout();
    display.prepare(&narrow, &timing(), |_, _| Ok(10)).unwrap();
    display.append("/D]E").unwrap();
    display.prepare(&narrow, &timing(), |_, _| Ok(10)).unwrap();
    display.advance(100, u32::MAX, mask).unwrap();
    display.set_speed(37);
    assert!(display.dirty_rectangle().is_some());
    assert!(
        display
            .page()
            .layout()
            .entries
            .iter()
            .any(|entry| entry.atom.is_none())
    );
    let mut defaults = defaults();
    defaults.text_sizes = [30; 5];
    defaults.ruby_sizes = [18; 5];
    defaults.text_color = 0xff99_aabb;
    let mut wide = layout();
    wide.width = 100;
    wide.normal_size = 30;
    display
        .reconfigure(
            defaults,
            [100, 64],
            DisplaySettings {
                reveal_width: 4,
                ..Default::default()
            },
            &wide,
            &timing(),
            |_, _| Ok(10),
        )
        .unwrap();
    assert_eq!(display.page().source(), r"\f+3;AB[C/D]E");
    assert_eq!(display.page().speed(), 37);
    assert_eq!(display.page().remaining(u32::MAX).unwrap(), 5);
    assert_eq!(display.dirty_rectangle(), None);
    assert_eq!(display.surface().size(), [100, 64]);
    for surface in [display.surface(), display.source_surface().unwrap()] {
        assert!(surface.pixels().iter().all(|&byte| byte == 0));
    }
    let entries = &display.page().layout().entries;
    assert_eq!(entries.len(), 5);
    assert!(entries.iter().all(|entry| entry.atom.is_some()));
    assert_eq!(entries[0].size[1], 33);
    assert_eq!(entries[3].size[1], 18);
    assert!(entries[3].group.unwrap().ruby);
    display.advance(0, u32::MAX, mask).unwrap();
    assert_eq!(display.advance(2, u32::MAX, mask).unwrap().completed, [0]);
    display.clear();
    display.append("Z").unwrap();
    display.prepare(&wide, &timing(), |_, _| Ok(10)).unwrap();
    assert_eq!(display.page().layout().entries[0].size[1], 30);
    assert_eq!(
        display.page().markup().atoms[0].style.text_color,
        0xff99_aabb
    );
    assert_eq!(display.page().speed(), 37);
}

#[test]
fn failed_reconfiguration_preserves_pixels_pending_source_and_progress_for_retry() {
    let mut display = display(4, "A");
    display.advance(0, u32::MAX, mask).unwrap();
    display.set_speed(37);
    display.append("[B").unwrap();
    let before = display.clone();
    for size in [[0, 64], [100, 64]] {
        assert!(
            display
                .reconfigure(
                    defaults(),
                    size,
                    *display.settings(),
                    &layout(),
                    &timing(),
                    |_, _| Err(Error::new(0, "metrics unavailable")),
                )
                .is_err()
        );
        assert_eq!(display, before);
    }
    display
        .reconfigure(
            defaults(),
            [100, 64],
            DisplaySettings::default(),
            &layout(),
            &timing(),
            |_, _| Ok(10),
        )
        .unwrap();
    assert_eq!(display.page().source(), "A[B");
    assert!(!display.page().is_dirty());
    assert_eq!(display.page().remaining(u32::MAX).unwrap(), 2);
    assert_eq!(display.page().speed(), 37);
    assert_eq!(display.source_surface(), None);
    assert!(display.surface().pixels().iter().all(|&byte| byte == 0));
    assert_eq!(display.dirty_rectangle(), None);
}
