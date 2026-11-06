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
