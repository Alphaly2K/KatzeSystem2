use katze_system2::{
    graphics::{Rect, Surface},
    message::{AtomKind, Layout, LayoutEntry, Reveal, RevealGlyph, TextEncoding},
};

#[test]
fn reveal_updates_only_alpha_and_keeps_untouched_pixels_when_time_recedes() {
    let reveal = Reveal::new(4).unwrap();
    assert_eq!(reveal.width(), 4);
    assert_eq!(reveal.gradient(), [255, 192, 128, 64]);
    let mut source = Surface::new([8, 3]).unwrap();
    source.clear(0x8033_2211);
    let mut target = source.clone();
    target.clear(0x1144_5566);
    Reveal::copy_colors(&mut target, &source).unwrap();
    assert!(
        target
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [0x11, 0x22, 0x33, 17])
    );
    let glyph = RevealGlyph {
        position: [0; 2],
        bounds: source.bounds(),
        start: 0,
    };
    let before = target.clone();
    assert!(!reveal.apply(&mut target, &source, &glyph, -1).unwrap());
    assert_eq!(target, before);
    assert!(!reveal.apply(&mut target, &source, &glyph, 4).unwrap());
    assert_eq!(
        (0..8)
            .map(|x| target.pixel(x, 0).unwrap() >> 24)
            .collect::<Vec<_>>(),
        [128, 96, 64, 32, 17, 17, 17, 17]
    );
    assert!(!reveal.apply(&mut target, &source, &glyph, 5).unwrap());
    assert_eq!(target.pixel(4, 0).unwrap() >> 24, 32);
    reveal.apply(&mut target, &source, &glyph, 4).unwrap();
    assert_eq!(target.pixel(4, 0).unwrap() >> 24, 32);
    assert!(reveal.apply(&mut target, &source, &glyph, 12).unwrap());
    assert_eq!(target.pixel(6, 1), Some(0x8033_2211));
    assert_eq!(target.pixel(7, 1), Some(0x1133_2211));
    assert_eq!(target.pixel(0, 2), Some(0x1133_2211));

    let before = target.clone();
    let negative = RevealGlyph {
        position: [-2, 0],
        bounds: Rect::from_xywh(0, 0, 5, 3),
        start: 0,
    };
    assert!(reveal.apply(&mut target, &source, &negative, 20).unwrap());
    assert_eq!(target, before);
    assert!(
        reveal
            .apply(&mut target, &Surface::new([1; 2]).unwrap(), &glyph, 20)
            .is_err()
    );
    assert_eq!(target, before);
}

#[test]
fn reveal_phase_uses_unsigned_wrapping_arithmetic_and_high_time_force() {
    assert_eq!(Reveal::phase(3, 20, 10), 12);
    assert_eq!(Reveal::phase(0, 20, 0), i32::MAX);
    assert_eq!(Reveal::phase(0x8000_0000, 0, 1), i32::MAX);
    assert_eq!(Reveal::phase(u32::MAX, 20, u32::MAX), i32::MAX);
    assert_eq!(Reveal::phase(1, u32::MAX, 1), -2);
    assert_eq!(Reveal::phase(0x4000_0001, 4, 3), 2);
    assert!(Reveal::new(0).is_err());
    assert!(Reveal::new(u32::MAX).is_err());
}

fn entry(character: char, line: u32, x: i32, width: i32) -> LayoutEntry {
    LayoutEntry {
        atom: None,
        kind: AtomKind::Character(character),
        encoding: TextEncoding::Cp932,
        position: [x, 0],
        size: [width, 20],
        line,
        group: None,
        display_anchor: None,
    }
}

#[test]
fn reveal_starts_use_record_order_line_minimum_and_internal_reset() {
    let mut layout = Layout {
        entries: vec![
            entry('A', 0, 10, 10),
            entry('B', 0, 30, 5),
            entry('C', 1, 5, 12),
            entry('D', 1, 20, 4),
            entry('r', 1, 8, 2),
            entry('E', 2, 9, 3),
        ],
        cursor: None,
        bottom: 0,
    };
    assert_eq!(
        layout.reveal_starts(),
        [Some(0), Some(20), Some(25), Some(40), Some(28), Some(30)]
    );
    layout.entries.insert(3, entry('\t', 0, 0, 0));
    assert_eq!(
        layout.reveal_starts(),
        [
            Some(0),
            Some(20),
            Some(25),
            None,
            Some(15),
            Some(3),
            Some(0)
        ]
    );

    layout.entries = vec![
        entry('A', 0, -1, 10),
        entry('B', 0, 5, 3),
        entry('C', 1, 0, 1),
    ];
    assert_eq!(layout.reveal_starts(), [Some(-6), Some(0), Some(3)]);
}
