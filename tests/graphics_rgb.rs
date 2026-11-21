use katze_system2::{
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
    host::graphics::Rect,
};

fn bitmap(size: [u32; 2], pixels: Vec<u8>, depth: u32, canvas: [u32; 2]) -> Image {
    Image::from_frame(DecodedFrame {
        id: 0,
        info: StandardInfo {
            size,
            bit_depth: depth,
            canvas_size: canvas,
            offset: [0; 2],
            base: [0; 2],
            reserved: 0,
        },
        pixels,
        attributes: Default::default(),
    })
    .unwrap()
}

#[test]
fn rgb_fill_and_blending_preserve_three_channel_rounding_and_row_copy_overlap() {
    for (color, expected) in [
        (0x00123456, [17, 231, 53, 255]),
        (0x01123456, [40, 194, 50, 255]),
        (0x7f123456, [40, 194, 50, 255]),
        (0xff123456, [18, 52, 86, 255]),
    ] {
        let mut target = bitmap([1, 1], vec![17, 231, 53, 0], 24, [1, 1]);
        assert!(target.fill_rect_from_bottom(Rect::from_xywh(-1, 0, 2, 1), color));
        assert_eq!(target.surface().pixels(), expected);
    }
    let mut target = bitmap([1, 1], vec![17, 231, 53, 255], 24, [1, 1]);
    let source = bitmap([1, 1], vec![201, 9, 99, 255], 32, [1, 1]);
    assert!(target.blend_from(&source, Rect::from_xywh(0, 0, 1, 1), [0, 0]));
    assert_eq!(target.surface().pixels(), &[200, 9, 98, 255]);

    let mut target = bitmap(
        [4, 1],
        vec![
            17, 231, 53, 255, 90, 16, 84, 255, 163, 57, 115, 255, 236, 98, 146, 255,
        ],
        24,
        [4, 1],
    );
    assert!(target.blend_within(Rect::from_xywh(0, 0, 3, 1), [1, 0]));
    assert_eq!(
        target.surface().pixels(),
        &[
            17, 231, 53, 255, 17, 231, 53, 255, 90, 16, 84, 255, 163, 57, 115, 255
        ]
    );
}

#[test]
fn rgb_resampling_and_vertical_blur_keep_opaque_views_and_diagnose_uninitialized_paths() {
    let source = bitmap([2, 1], vec![10, 20, 30, 0, 100, 110, 120, 0], 24, [2, 1]);
    let mut target = bitmap([4, 1], vec![0; 16], 24, [4, 1]);
    target
        .copy_scaled_from(
            &source,
            Rect::from_xywh(0, 0, 2, 1),
            Rect::from_xywh(0, 0, 4, 1),
        )
        .unwrap();
    assert_eq!(
        target.surface().pixels(),
        &[
            10, 20, 30, 255, 55, 65, 75, 255, 100, 110, 120, 255, 100, 110, 120, 255
        ]
    );
    let mut target = bitmap([1, 2], vec![50, 100, 150, 0, 150, 200, 250, 0], 24, [1, 2]);
    let before = target.clone();
    assert!(!target.blur([-1, 1]).unwrap());
    assert_eq!(target, before);
    target.blur([0, 1]).unwrap();
    assert_eq!(
        target.surface().pixels(),
        &[75, 125, 175, 255, 125, 175, 225, 255]
    );
    assert!(
        target
            .blur([1, 0])
            .unwrap_err()
            .message
            .contains("未初始化")
    );
    let mut target = bitmap([1, 2], vec![0; 8], 24, [2, 3]);
    assert!(target.blur([0, 1]).unwrap_err().message.contains("行填充"));
}
