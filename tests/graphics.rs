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
    for size in [[4097, 4096], [4752, 5277], [8192, 4096]] {
        assert_eq!(
            Surface::from_bgra(size, Vec::new()).unwrap_err().message,
            "渲染面像素长度与尺寸不符"
        );
    }
    assert!(Surface::new([8193, 4096]).is_err());
    assert!(Surface::new([u32::MAX, 0]).is_err());
}

#[test]
fn image_rectangle_fills_clip_before_converting_bottom_origin_coordinates() {
    let mut surface = Surface::new([3, 4]).unwrap();
    assert!(surface.fill_rect_from_bottom(Rect::from_xywh(1, 0, 1, 1), 0x0011_2233));
    assert_eq!(surface.pixel(1, 3), Some(0x0011_2233));
    assert_eq!(surface.pixel(1, 0), Some(0));
    assert!(surface.fill_rect_from_bottom(Rect::from_xywh(-1, 2, 3, 4), 0x8044_5566));
    for y in 0..4 {
        for x in 0..3 {
            let expected = if x < 2 && y < 2 {
                0x8044_5566
            } else if x == 1 && y == 3 {
                0x0011_2233
            } else {
                0
            };
            assert_eq!(surface.pixel(x, y), Some(expected));
        }
    }
    assert!(!surface.fill_rect_from_bottom(Rect::from_xywh(0, 4, 1, 1), u32::MAX));
    assert!(!surface.fill_rect_from_bottom(Rect::from_xywh(0, 3, 1, -1), u32::MAX));
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

#[test]
fn ordinary_alpha_blending_preserves_integer_rounding_and_transparent_colors() {
    for (source, destination, opacity, expected) in [
        (0x0044_88ccu32, 0x0077_4433u32, 255, 0x0077_4433),
        (0x0156_bd23, 0x00ed_9864, 255, 0x0156_bd23),
        (0xff92_2b75, 0x806d_a102, 255, 0xff92_2b75),
        (0x8081_5d4c, 0xff7e_2bb3, 255, 0xff7f_447f),
        (0x808a_344c, 0x80b9_dcb3, 255, 0xc198_6b6d),
        (0x018b_1423, 0x0141_c964, 255, 0x0100_0000),
        (0xfe42_c91e, 0xfe67_6ad1, 255, 0xff41_c71e),
        (0xff89_5475, 0xff31_f002, 128, 0xff5d_a23b),
        (0x8055_dd4c, 0x7f65_abb3, 128, 0x9f5d_bd89),
        (0xff66_ab75, 0x0055_2102, 0, 0x0055_2102),
        (0x80ff_ffff, 0x80ff_ffff, 255, 0xc1fd_fdfd),
        (0xff11_2233, 0, 0, 0x0011_2233),
    ] {
        let source = Surface::from_bgra([1, 1], source.to_le_bytes().to_vec()).unwrap();
        let mut target = Surface::from_bgra([1, 1], destination.to_le_bytes().to_vec()).unwrap();
        assert!(target.blend_from(&source, source.bounds(), [0, 0], opacity));
        assert_eq!(target.pixel(0, 0), Some(expected));
    }
}

#[test]
fn ordinary_blending_clips_independently_and_extends_only_into_zero_neighbors() {
    let source = Surface::from_bgra(
        [3, 2],
        [0x0011_2233u32, 0x8044_5566, 0xff77_8899, 1, 2, 3]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )
    .unwrap();
    let mut target = Surface::new([5, 4]).unwrap();
    target.fill_rect(Rect::from_xywh(0, 2, 1, 1), 0x0012_3456);
    target.fill_rect(Rect::from_xywh(4, 1, 1, 1), 0x0100_0000);
    assert!(target.blend_from(&source, source.bounds(), [1, 1], 0));
    let expected = [
        [0; 5],
        [
            0x0011_2233u32,
            0x0011_2233,
            0x0044_5566,
            0x0077_8899,
            0x0100_0000,
        ],
        [0x0012_3456, 1, 2, 3, 3],
        [0; 5],
    ];
    assert_eq!(
        target.pixels(),
        expected
            .into_iter()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );

    let mut target = Surface::new([4, 3]).unwrap();
    assert!(target.blend_from(&source, source.bounds(), [-1, -1], 0));
    assert_eq!(target.pixel(0, 0), Some(0x0011_2233));
    assert_eq!(target.pixel(1, 0), Some(0x0044_5566));
    assert_eq!(target.pixel(2, 0), Some(0x0044_5566));
    assert_eq!(target.pixel(3, 0), Some(0));
    assert!(target.pixels()[16..].iter().all(|&byte| byte == 0));

    target.clear(0);
    assert!(target.blend_from(&source, Rect::from_xywh(-2, -1, 5, 4), [1, 1], 0));
    assert_eq!(target.pixel(0, 1), Some(0x0011_2233));
    assert_eq!(target.pixel(1, 1), Some(0x0011_2233));
    assert_eq!(target.pixel(3, 2), Some(3));
    let before = target.clone();
    assert!(!target.blend_from(&source, source.bounds(), [4, 0], 255));
    assert!(!target.blend_from(&source, Rect::from_xywh(0, 0, -1, 2), [0, 0], 255));
    assert_eq!(target, before);
}
