use katze_system2::graphics::{Rect, Surface};

#[test]
fn fills_clip_preserve_raw_argb_and_leave_other_pixels_untouched() {
    let pixels = [0x55, 0x66, 0x77, 0x88].repeat(6);
    let mut surface = Surface::from_bgra([3, 2], pixels).unwrap();
    assert_eq!(surface.size(), [3, 2]);
    assert_eq!(surface.stride(), 12);
    assert!(surface.fill_rect(Rect::from_xywh(-1, 0, 3, 2), 0x0011_2233));
    for y in 0..2 {
        assert_eq!(surface.pixel(0, y), Some(0x0011_2233));
        assert_eq!(surface.pixel(1, y), Some(0x0011_2233));
        assert_eq!(surface.pixel(2, y), Some(0x8877_6655));
    }
    assert_eq!(&surface.pixels()[..4], [0x33, 0x22, 0x11, 0]);
    let before = surface.pixels().to_vec();
    for rect in [
        Rect::from_xywh(0, 0, -1, 2),
        Rect::from_xywh(0, 2, 1, 1),
        Rect::from_xywh(i32::MAX, 0, 2, 1),
    ] {
        assert!(!surface.fill_rect(rect, 0));
    }
    assert_eq!(surface.pixels(), before);
    assert_eq!(surface.pixel(3, 0), None);
    assert_eq!(surface.pixel(0, 2), None);
    surface.clear(0x7f12_3456);
    assert_eq!(surface.pixels(), [0x56, 0x34, 0x12, 0x7f].repeat(6));
    surface.pixels_mut()[12..16].copy_from_slice(&[1, 2, 3, 4]);
    assert_eq!(surface.pixel(0, 0), Some(0x7f12_3456));
    assert_eq!(surface.pixel(0, 1), Some(0x0403_0201));
}

#[test]
fn surfaces_allow_empty_dimensions_and_reject_invalid_storage() {
    for size in [[0, 0], [0, 3], [3, 0]] {
        let mut surface = Surface::new(size).unwrap();
        surface.clear(u32::MAX);
        assert!(surface.pixels().is_empty());
        assert_eq!(surface.pixel(0, 0), None);
        assert!(!surface.fill_rect(Rect::from_xywh(-1, -1, 5, 5), 1));
    }
    assert!(Surface::from_bgra([1, 1], vec![0; 3]).is_err());
    assert!(Surface::from_bgra([1, 1], vec![0; 5]).is_err());
    assert!(Surface::new([4097, 4096]).is_err());
    assert!(Surface::new([u32::MAX, 0]).is_err());
}

#[test]
fn image_copy_clips_both_ends_without_shifting_the_requested_mapping() {
    let bytes: Vec<_> = [0x0011_2233u32, 0x7f44_5566, 0xff77_8899, 1, 2, 3]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let source = Surface::from_bgra([3, 2], bytes).unwrap();
    for rect in [
        source.bounds(),
        Rect::from_xywh(-2, -1, 5, 4),
        Rect::from_xywh(1, 0, 4, 3),
        Rect::from_xywh(1, 1, -1, 2),
        Rect::from_xywh(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
    ] {
        for x in [i32::MIN, -4, -1, 0, 2, i32::MAX] {
            for y in [i32::MIN, -4, -1, 0, 2, i32::MAX] {
                let mut destination = Surface::new([4, 3]).unwrap();
                destination.clear(0xaabb_ccdd);
                let copied = destination.copy_from(&source, rect, [x, y]);
                let mut expected_copy = false;
                for dy in 0..3 {
                    for dx in 0..4 {
                        let sx = i64::from(rect.left) + i64::from(dx) - i64::from(x);
                        let sy = i64::from(rect.top) + i64::from(dy) - i64::from(y);
                        let expected = if sx >= i64::from(rect.left)
                            && sx < i64::from(rect.right)
                            && sy >= i64::from(rect.top)
                            && sy < i64::from(rect.bottom)
                            && (0..3).contains(&sx)
                            && (0..2).contains(&sy)
                        {
                            expected_copy = true;
                            source.pixel(sx as u32, sy as u32).unwrap()
                        } else {
                            0xaabb_ccdd
                        };
                        assert_eq!(destination.pixel(dx, dy), Some(expected));
                    }
                }
                assert_eq!(copied, expected_copy);
            }
        }
    }
    assert!(
        !Surface::new([0, 1])
            .unwrap()
            .copy_from(&source, source.bounds(), [0, 0])
    );
}
