use katze_system2::hg2;

#[test]
fn thumbnail_encoding_crops_transparency_and_preserves_top_down_pixels() {
    let mut pixels = vec![0; 4 * 3 * 4];
    pixels[20..28].copy_from_slice(&[11, 22, 33, 44, 55, 66, 77, 255]);
    pixels[36..44].copy_from_slice(&[99, 88, 77, 255, 9, 8, 7, 6]);
    let bytes = hg2::encode_frame([4, 3], &pixels).unwrap();
    let frame = hg2::decode_frame(&bytes).unwrap();
    assert_eq!(frame.id, 0);
    assert_eq!(frame.info.size, [2, 2]);
    assert_eq!(frame.info.canvas_size, [4, 3]);
    assert_eq!(frame.info.offset, [1, 1]);
    assert_eq!(frame.info.base, [0, 0]);
    assert_eq!(frame.info.reserved, 1);
    assert_eq!(
        frame.pixels,
        [11, 22, 33, 44, 55, 66, 77, 255, 99, 88, 77, 255, 9, 8, 7, 6]
    );

    let mut malformed = bytes.clone();
    for (at, value) in [
        (4, 11u32),
        (8, 0x21),
        (12, u32::MAX),
        (20, 16),
        (32, u32::MAX),
        (36, u32::MAX),
        (44, 0),
        (76, 40),
    ] {
        malformed[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(hg2::decode_frame(&malformed).is_err(), "{at}");
        malformed.copy_from_slice(&bytes);
    }
    assert!(hg2::decode_frame(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn legacy_headers_and_linked_frames_preserve_selection_and_pixels() {
    let pixel = [20, 49, 127, 202];
    let encoded = hg2::encode_frame_with_depth([1, 1], &pixel, 24).unwrap();
    let mut legacy = encoded[..48].to_vec();
    legacy[8..12].copy_from_slice(&0x10u32.to_le_bytes());
    legacy[22..24].copy_from_slice(&5u16.to_le_bytes());
    legacy.extend_from_slice(&encoded[88..]);
    let decoded = hg2::decode_frame_id(&legacy, u32::MAX).unwrap().unwrap();
    assert_eq!(decoded.id, 0);
    assert_eq!(decoded.pixels, [0xa5, 0x8c, 0xff, 255]);
    assert_eq!(decoded.info.canvas_size, [1, 1]);

    let encoded = hg2::encode_frame([1, 1], &pixel).unwrap();
    for version in [0x20u32, 0x25] {
        let extra = usize::from(version == 0x25) * 8;
        let make_frame = |id: u32, x: i32, padding: usize, linked: bool| {
            let mut frame = encoded[12..80 + extra].to_vec();
            frame[0x24..0x28].copy_from_slice(&((32 + extra + padding) as u32).to_le_bytes());
            frame[0x28..0x2c].copy_from_slice(&id.to_le_bytes());
            frame[0x34..0x38].copy_from_slice(&x.to_le_bytes());
            if version == 0x25 {
                frame[0x44..0x48].copy_from_slice(&3i32.to_le_bytes());
            }
            frame.resize(frame.len() + padding, 0);
            frame.extend_from_slice(&encoded[88..]);
            if linked {
                let size = frame.len() as u32;
                frame[0x40..0x44].copy_from_slice(&size.to_le_bytes());
            }
            frame
        };
        let mut data = encoded[..12].to_vec();
        data[8..12].copy_from_slice(&version.to_le_bytes());
        let mut skipped = make_frame(7, 11, 0, true);
        skipped[68 + extra] = 0;
        data.extend(skipped);
        data.extend(make_frame(0, -2, 4, true));
        data.extend(make_frame(0, 99, 0, false));
        let decoded = hg2::decode_frame_id(&data, 0).unwrap().unwrap();
        assert_eq!(decoded.pixels, pixel);
        assert_eq!(decoded.info.offset, [-2, 0]);
        assert_eq!(decoded.info.base, [if version == 0x25 { 3 } else { 0 }, 0]);
        assert!(hg2::decode_frame_id(&data, 7).is_err());
        assert!(hg2::decode_frame_id(&data, 1).unwrap().is_none());
    }
}

#[test]
fn rgb_frames_keep_all_pixels_and_the_final_partial_plane_group() {
    for size in [[1, 1], [3, 2], [7, 49], [17, 21]] {
        let pixels: Vec<_> = (0..size[0] * size[1])
            .flat_map(|i| [(i * 17) as u8, (i * 37) as u8, (i * 53) as u8, 0])
            .collect();
        let frame =
            hg2::decode_frame(&hg2::encode_frame_with_depth(size, &pixels, 24).unwrap()).unwrap();
        assert_eq!(frame.info.bit_depth, 24);
        assert_eq!(frame.info.size, size);
        assert_eq!(frame.info.canvas_size, size);
        assert_eq!(frame.info.offset, [0; 2]);
        assert_eq!(frame.info.reserved, 0);
        for (actual, expected) in frame.pixels.chunks_exact(4).zip(pixels.chunks_exact(4)) {
            assert_eq!(&actual[..3], &expected[..3]);
            assert_eq!(actual[3], 255);
        }
    }
    assert!(hg2::encode_frame_with_depth([1, 1], &[0; 4], 16).is_err());
}

#[test]
fn empty_and_opaque_thumbnails_keep_canvas_size_and_alpha_classification() {
    for (size, value, cropped, alpha) in [
        ([1, 1], 0, [1, 1], 1),
        ([7, 5], 0, [2, 2], 1),
        ([7, 5], 255, [7, 5], 0),
    ] {
        let pixels = vec![value; (size[0] * size[1] * 4) as usize];
        let frame = hg2::decode_frame(&hg2::encode_frame(size, &pixels).unwrap()).unwrap();
        assert_eq!(frame.info.size, cropped);
        assert_eq!(frame.info.canvas_size, size);
        assert_eq!(frame.info.reserved, alpha);
        assert_eq!(
            frame.pixels,
            vec![value; (cropped[0] * cropped[1] * 4) as usize]
        );
    }
    assert!(hg2::encode_frame([0, 1], &[]).is_err());
    assert!(hg2::encode_frame([1, 1], &[0; 3]).is_err());
}
