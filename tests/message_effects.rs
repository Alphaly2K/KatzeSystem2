use katze_system2::{
    graphics::{Rect, Surface},
    message::{GlyphEffects, GlyphMask, GlyphPaint},
};

fn alpha_surface(size: [u32; 2], alphas: &[u8]) -> Surface {
    Surface::from_bgra(size, alphas.iter().flat_map(|&a| [1, 2, 3, a]).collect()).unwrap()
}

#[test]
fn outline_accumulates_neighbors_and_preserves_the_width_based_column_copy() {
    let effects = GlyphEffects {
        outline: true,
        ..Default::default()
    };
    let source = alpha_surface([3; 2], &[0, 10, 0, 20, 30, 40, 0, 50, 0]);
    let mut target = Surface::new([3; 2]).unwrap();
    assert!(
        effects
            .apply(&mut target, &source, 20, 0x125d_3b19)
            .unwrap()
    );
    for (pixel, alpha) in target
        .pixels()
        .chunks_exact(4)
        .zip([60, 100, 80, 110, 150, 130, 100, 140, 120])
    {
        assert_eq!(pixel, [0x19, 0x3b, 0x5d, alpha]);
    }
    let source = alpha_surface([2; 2], &[128; 4]);
    let mut target = Surface::new([2; 2]).unwrap();
    effects.apply(&mut target, &source, 20, 0).unwrap();
    assert!(target.pixels().chunks_exact(4).all(|p| p[3] == 255));

    let source = alpha_surface([1, 4], &[10, 20, 30, 40]);
    let mut target = Surface::new([1, 4]).unwrap();
    effects.apply(&mut target, &source, 20, 0).unwrap();
    assert_eq!(
        target
            .pixels()
            .chunks_exact(4)
            .map(|p| p[3])
            .collect::<Vec<_>>(),
        [10, 10, 0, 0]
    );

    let source = Surface::new([4, 3]).unwrap();
    let mut target = source.clone();
    target.clear(0x1234_5678);
    let before = target.clone();
    assert!(effects.apply(&mut target, &source, 20, 0).is_err());
    assert_eq!(target, before);
}

#[test]
fn offset_shadow_uses_prior_destination_alpha_and_horizontal_feedback() {
    let source = alpha_surface([4, 1], &[1, 3, 5, 7]);
    let mut target = alpha_surface([4, 1], &[2, 4, 8, 16]);
    let effects = GlyphEffects {
        shadow: true,
        shadow_offset: [1, 0],
        ..Default::default()
    };
    effects
        .apply(&mut target, &source, 20, 0x0019_3b5d)
        .unwrap();
    assert_eq!(target.pixel(0, 0), Some(0x0203_0201));
    assert_eq!(target.pixel(1, 0), Some(0x0719_3b5d));
    assert_eq!(target.pixel(2, 0), Some(0x1219_3b5d));
    assert_eq!(target.pixel(3, 0), Some(0x2719_3b5d));

    let before = target.clone();
    let invalid = GlyphEffects {
        shadow_offset: [-3, 1],
        ..effects
    };
    assert!(invalid.apply(&mut target, &source, 19, 0).is_err());
    assert_eq!(target, before);
    let disabled = GlyphEffects {
        shadow: false,
        ..invalid
    };
    assert!(!disabled.apply(&mut target, &source, 19, 0).unwrap());
    assert_eq!(target, before);
}

#[test]
fn glyph_paint_uses_the_small_size_offset_but_retains_configured_bounds() {
    let mask = GlyphMask::new([1; 2], [1; 2], 1, vec![32]).unwrap();
    let mut paint = GlyphPaint {
        color: 0x1156_3412,
        shadow_color: 0x225d_3b19,
        font_size: 19,
        double_coverage: false,
        effects: GlyphEffects {
            shadow: true,
            shadow_offset: [3, 1],
            ..Default::default()
        },
    };
    for (size, shadow_x) in [(19, 3), (20, 4)] {
        paint.font_size = size;
        let mut target = Surface::new([8; 2]).unwrap();
        assert_eq!(
            mask.paint(&mut target, &paint).unwrap(),
            Rect::from_xywh(0, 0, 7, 5)
        );
        assert_eq!(target.pixel(1, 1), Some(0x7f12_3456));
        assert_eq!(target.pixel(shadow_x, 2), Some(0x7f5d_3b19));
        assert_eq!(target.pixel(7, 7), Some(0x005d_3b19));
    }

    paint.effects.shadow = false;
    paint.double_coverage = true;
    let mut target = Surface::new([8; 2]).unwrap();
    target.clear(0xffff_ffff);
    mask.paint(&mut target, &paint).unwrap();
    assert_eq!(target.pixel(1, 1), Some(0xfe12_3456));
    assert_eq!(target.pixel(0, 0), Some(0));
    assert_eq!(target.pixel(7, 7), Some(0xffff_ffff));
}
