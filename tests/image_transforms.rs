use katze_system2::{
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
};

fn image() -> Image {
    Image::from_frame(DecodedFrame {
        id: 7,
        info: StandardInfo {
            size: [3, 2],
            bit_depth: 32,
            offset: [-2, 5],
            canvas_size: [11, 13],
            reserved: 1,
            base: [3, -4],
        },
        pixels: (1..=6).flat_map(|value| [value, 0, 0, 64]).collect(),
        attributes: [(1, [7, 8, 9, 10, 11])].into(),
    })
    .unwrap()
}

#[test]
fn orthogonal_transforms_keep_pixels_and_attribute_coordinates_in_the_same_canvas() {
    for (operation, size, pixels, offset, base, attribute) in [
        (1, [3, 2], [3, 2, 1, 6, 5, 4], [10, 5], [8, -4], [4, 8]),
        (0, [3, 2], [4, 5, 6, 1, 2, 3], [-2, 6], [3, 17], [7, 5]),
        (90, [2, 3], [3, 6, 2, 5, 1, 4], [6, -2], [17, 3], [5, 7]),
        (180, [3, 2], [6, 5, 4, 3, 2, 1], [10, 6], [8, 17], [4, 5]),
        (270, [2, 3], [4, 1, 5, 2, 6, 3], [5, 10], [-4, 8], [8, 4]),
    ] {
        let mut image = image();
        if operation <= 1 {
            image.flip(operation != 0);
        } else {
            assert!(image.rotate_right_angle(operation).unwrap());
        }
        let info = image.info();
        assert_eq!(info.size, size);
        assert_eq!(info.offset, offset);
        assert_eq!(info.base, base);
        assert_eq!(
            info.canvas_size,
            if size == [3, 2] { [11, 13] } else { [13, 11] }
        );
        assert_eq!(image.id(), 7);
        assert_eq!(info.reserved, 1);
        assert_eq!(
            image.attribute(1),
            Some([attribute[0], attribute[1], 9, 10, 11])
        );
        assert_eq!(
            image.surface().pixels(),
            pixels
                .into_iter()
                .flat_map(|value| [value, 0, 0, 64])
                .collect::<Vec<_>>()
        );
    }
    for angle in [0, -90, 360, 450, i32::MIN] {
        let mut image = image();
        let before = image.clone();
        assert_eq!(image.rotate_right_angle(angle).unwrap(), angle == 0);
        assert_eq!(image, before);
    }
    let mut empty = Image::new([0, 3]).unwrap();
    empty.flip(false);
    assert!(empty.rotate_right_angle(90).unwrap());
    assert_eq!(empty.info().size, [3, 0]);
    assert!(empty.surface().pixels().is_empty());
}

#[test]
fn rgb_percentages_preserve_alpha_and_wrap_before_clamping() {
    let mut image = Image::new([2, 1]).unwrap();
    image
        .pixels_mut()
        .copy_from_slice(&[50, 100, 200, 64, 2, 3, 4, 127]);
    image.adjust_rgb_percent([50, 125, 200]);
    assert_eq!(image.surface().pixels(), [100, 125, 100, 64, 4, 3, 2, 127]);
    image.adjust_rgb_percent([0x8000_0000, u32::MAX, 0x8000_0000]);
    assert_eq!(image.surface().pixels(), [0, 255, 0, 64, 0, 255, 0, 127]);
}

#[test]
fn blur_keeps_transparent_colors_and_integer_rounding_between_passes() {
    let make = |pixels| {
        Image::from_frame(DecodedFrame {
            id: 1,
            info: StandardInfo {
                size: [3, 1],
                bit_depth: 32,
                offset: [0; 2],
                canvas_size: [3, 1],
                reserved: 1,
                base: [-4, 5],
            },
            pixels,
            attributes: [(1, [1, 2, 3, 4, 5])].into(),
        })
        .unwrap()
    };
    let mut image = make(vec![20, 40, 60, 255, 200, 180, 160, 0, 100, 80, 60, 128]);
    assert!(image.blur([1, 0]).unwrap());
    assert_eq!(
        image.surface().pixels(),
        [20, 40, 60, 191, 55, 57, 60, 96, 99, 79, 59, 96]
    );
    assert_eq!(image.info().base, [-4, 5]);
    assert_eq!(image.attribute(1), Some([1, 2, 3, 4, 5]));

    let mut image = make(vec![100, 80, 60, 1, 200, 200, 200, 0, 0, 0, 0, 0]);
    image.blur([1, 1]).unwrap();
    assert_eq!(
        image.surface().pixels(),
        [98, 78, 58, 1, 100, 80, 60, 0, 0, 0, 0, 0]
    );
    let mut clamped = image.clone();
    image.blur([300, i32::MIN]).unwrap();
    clamped.blur([255, 0]).unwrap();
    assert_eq!(image, clamped);

    let mut invalid = self::image();
    let before = invalid.clone();
    assert!(invalid.blur([1, 1]).is_err());
    assert_eq!(invalid, before);
    assert!(!Image::new([0, 3]).unwrap().blur([1, 1]).unwrap());
}

#[test]
fn canvas_resize_keeps_bottom_sampling_and_distinct_metadata_conversions() {
    let mut image = image();
    assert!(image.resize_canvas([22, 39]).unwrap());
    assert_eq!(image.info().size, [6, 6]);
    assert_eq!(image.info().canvas_size, [22, 39]);
    assert_eq!(image.info().offset, [0, 15]);
    assert_eq!(image.info().base, [6, -11]);
    assert_eq!(image.attribute(1), Some([14, 24, 18, 30, 11]));
    assert_eq!(image.id(), 7);
    assert_eq!(image.info().reserved, 1);
    for (y, values) in [(0, [1, 1, 2, 2, 3, 3]), (5, [4, 4, 5, 5, 6, 6])] {
        for (x, value) in values.into_iter().enumerate() {
            assert_eq!(image.surface().pixel(x as u32, y), Some(64 << 24 | value));
        }
    }
    assert_eq!(image.surface().pixel(0, 2), Some(64 << 24 | 1));
    assert!(image.resize_canvas([0, 39]).unwrap());
    assert_eq!(image.info().size, [0, 6]);
    assert_eq!(image.info().base, [0, -10]);
    let before = image.clone();
    assert!(!image.resize_canvas([0, 39]).unwrap());
    assert_eq!(image, before);

    for (width, expected) in [(8, [0; 8]), (9, [2, 0, 0, 0, 6, 0, 0, 0])] {
        let mut image = Image::from_frame(DecodedFrame {
            id: 0,
            info: StandardInfo {
                size: [width, 1],
                canvas_size: [width, 1],
                offset: [0; 2],
                base: [0; 2],
                bit_depth: 32,
                reserved: 1,
            },
            pixels: (1..=width)
                .flat_map(|value| [value as u8, 0, 0, 0])
                .collect(),
            attributes: Default::default(),
        })
        .unwrap();
        image.resize_canvas([2, 1]).unwrap();
        assert_eq!(image.surface().pixels(), expected);
    }
}

#[test]
fn scaled_self_copy_uses_row_moves_and_immediate_resampling_reads() {
    use katze_system2::graphics::Rect;

    let mut image = Image::new([3, 2]).unwrap();
    image.pixels_mut().copy_from_slice(
        &(1..=6)
            .flat_map(|value| [value, 0, 0, 255])
            .collect::<Vec<_>>(),
    );
    image
        .copy_scaled_within(Rect::from_xywh(0, 0, 2, 2), Rect::from_xywh(1, 0, 2, 2))
        .unwrap();
    assert_eq!(
        image.surface().pixels(),
        [1, 1, 2, 4, 4, 5]
            .into_iter()
            .flat_map(|value| [value, 0, 0, 255])
            .collect::<Vec<_>>()
    );
    let mut image = Image::new([3, 1]).unwrap();
    image
        .pixels_mut()
        .copy_from_slice(&[0, 0, 0, 255, 100, 0, 0, 255, 200, 0, 0, 255]);
    image
        .copy_scaled_within(Rect::from_xywh(0, 0, 3, 1), Rect::from_xywh(1, 0, 2, 1))
        .unwrap();
    assert_eq!(
        image.surface().pixels(),
        [0, 0, 0, 255, 0, 0, 0, 255, 100, 0, 0, 255]
    );
}
