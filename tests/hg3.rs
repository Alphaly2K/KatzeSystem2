use katze_system2::hg3;
use std::io::Write;

fn image() -> Vec<u8> {
    let mut out = b"HG-3".to_vec();
    out.extend(12u32.to_le_bytes());
    out.extend(0x300u32.to_le_bytes());
    for (i, id, x) in [(0, 7u32, 11i32), (1, 9, -3), (2, 7, 99)] {
        out.extend((if i == 2 { 0 } else { 88u32 }).to_le_bytes());
        out.extend(id.to_le_bytes());
        out.extend(b"unknown\0");
        out.extend(24u32.to_le_bytes());
        out.extend(8u32.to_le_bytes());
        out.extend([0; 8]);
        out.extend(b"stdinfo\0");
        out.extend(0u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        for value in [123u32, 45, 32, x as u32, 22, 1000, 600, 0, 4, 5] {
            out.extend(value.to_le_bytes());
        }
    }
    out
}

#[test]
fn metadata_preserves_frame_order_and_uses_first_duplicate_id() {
    let metadata = hg3::Metadata::parse(&image()).unwrap();
    assert_eq!(metadata.frames.len(), 3);
    let frame = metadata.frame(7).unwrap();
    assert_eq!(frame.offset, 12);
    let info = frame.info.unwrap();
    assert_eq!(info.size, [123, 45]);
    assert_eq!(info.offset, [11, 22]);
    assert_eq!(info.canvas_size, [1000, 600]);
    assert_eq!(info.base, [4, 5]);
    assert_eq!(info.bit_depth, 32);
    assert_eq!(metadata.frame(9).unwrap().info.unwrap().offset[0], -3);
    assert_eq!(metadata.frames[2].info.unwrap().offset[0], 99);
    assert_eq!(metadata.frame(0), None);
}

#[test]
fn metadata_rejects_truncated_or_overlapping_frame_and_tag_ranges() {
    let valid = image();
    for end in [0, 4, 12, 19, 30, valid.len() - 1] {
        assert!(hg3::Metadata::parse(&valid[..end]).is_err());
    }
    for (at, value) in [(12, 7u32), (12, u32::MAX), (28, 16), (32, 89), (56, 39)] {
        let mut invalid = valid.clone();
        invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(hg3::Metadata::parse(&invalid).is_err(), "{at} {value}");
    }
}

const LITERALS: &[u8] = &[
    0x50, 0x55, 0x5a, 0x59, 0xaa, 0xf0, 0x49, 0x55, 0xb1, 0x22, 0xaa, 0xb0, 0x8d, 0xaa, 0x1b, 0xd3,
    0xff, 0xcf, 0x22, 0xaf, 0xff, 0x50,
];
const COMMANDS: &[u8] = &[0x61, 0x40, 0x96, 0x01];

fn pixel_image(size: [u32; 2], literals: &[u8], commands: &[u8]) -> Vec<u8> {
    fn compress(bytes: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    let data = compress(literals);
    let control = compress(commands);
    let mut out = b"HG-3".to_vec();
    for value in [12u32, 0x300, 0, 0x8000_0001] {
        out.extend(value.to_le_bytes());
    }
    out.extend(b"stdinfo\0");
    for value in [
        56u32,
        40,
        size[0],
        size[1],
        32,
        u32::MAX,
        7,
        20,
        30,
        0,
        2,
        3,
    ] {
        out.extend(value.to_le_bytes());
    }
    out.extend(b"img0000\0");
    for value in [
        0,
        (24 + data.len() + control.len()) as u32,
        0,
        size[1],
        data.len() as u32,
        literals.len() as u32,
        control.len() as u32,
        commands.len() as u32,
    ] {
        out.extend(value.to_le_bytes());
    }
    out.extend(data);
    out.extend(control);
    out
}

#[test]
fn native_pixels_restore_bit_planes_signed_deltas_and_top_down_bgra() {
    let bytes = pixel_image([2, 3], LITERALS, COMMANDS);
    let frame = hg3::decode_frame(&bytes, 0x8000_0001u32 as i32)
        .unwrap()
        .unwrap();
    assert_eq!(frame.id, 0x8000_0001);
    assert_eq!(frame.info.size, [2, 3]);
    assert_eq!(frame.info.offset, [-1, 7]);
    assert_eq!(frame.info.canvas_size, [20, 30]);
    assert_eq!(frame.info.base, [2, 3]);
    assert_eq!(
        frame.pixels,
        [
            11, 22, 33, 44, 55, 66, 77, 88, 89, 90, 91, 92, 255, 0, 128, 127, 1, 2, 3, 4, 5, 6, 7,
            8,
        ]
    );
    assert_eq!(hg3::decode_frame(&bytes, 0).unwrap(), None);

    let zero = pixel_image([1, 1], &[], &[0x08, 0x01]);
    assert_eq!(
        hg3::decode_frame(&zero, 0x8000_0001u32 as i32)
            .unwrap()
            .unwrap()
            .pixels,
        [0; 4]
    );
}

#[test]
fn attributes_keep_integer_prefix_ids_last_values_and_decoded_image_records() {
    let mut bytes = pixel_image([1, 1], &[], &[0x08, 0x01]);
    let next = bytes.len() - 76;
    bytes[84..88].copy_from_slice(&(next as u32).to_le_bytes());
    let names = [
        *b"ats-1\0\0\0",
        *b"ats0x1\0\0",
        *b"ats0001\0",
        *b"ATS0001\0",
        *b"ats1.5\0\0",
        *b"ats 1\0\0\0",
        *b"ats\0\0\0\0\0",
        *b"ats12345",
    ];
    for (index, name) in names.into_iter().enumerate() {
        bytes.extend(name);
        bytes.extend((if index == names.len() - 1 { 0u32 } else { 40 }).to_le_bytes());
        bytes.extend(24u32.to_le_bytes());
        bytes.extend(
            [index as u32, u32::MAX, 0x80000000, 3, 4, 0xdeadbeef]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
    }
    let metadata = hg3::Metadata::parse(&bytes).unwrap();
    let records = &metadata.frames[0].attributes;
    assert_eq!(records.len(), 3);
    assert_eq!(records[&1], [2, u32::MAX, 0x80000000, 3, 4]);
    assert_eq!(records[&65535][0], 0);
    assert_eq!(records[&12345][0], 7);
    let frame = hg3::decode_frame(&bytes, 0x80000001u32 as i32)
        .unwrap()
        .unwrap();
    assert_eq!(&frame.attributes, records);
    let image = katze_system2::graphics::Image::from_frame(frame).unwrap();
    assert_eq!(image.attribute(65537), Some(records[&1]));
    assert_eq!(image.attribute(u32::MAX), Some(records[&65535]));
    assert_eq!(image.attribute(7), None);
    let last = bytes.len() - 40;
    let mut continued = bytes.clone();
    continued[last + 8..last + 12].copy_from_slice(&48u32.to_le_bytes());
    continued.extend([0; 8]);
    continued.extend(b"unknown\0");
    continued.extend([0; 8]);
    let metadata = hg3::Metadata::parse(&continued).unwrap();
    assert_eq!(metadata.frames[0].attributes[&(123450u32 as u16)][0], 7);
    assert!(!metadata.frames[0].attributes.contains_key(&12345));
    bytes[last + 12..last + 16].copy_from_slice(&19u32.to_le_bytes());
    assert!(hg3::Metadata::parse(&bytes).is_err());
}

#[test]
fn padded_run_buffers_keep_the_full_plane_stride_and_return_only_visible_pixels() {
    let bytes = pixel_image([1, 1], LITERALS, COMMANDS);
    let frame = hg3::decode_frame(&bytes, 0x8000_0001u32 as i32)
        .unwrap()
        .unwrap();
    assert_eq!(frame.info.size, [1, 1]);
    assert_eq!(frame.pixels, [11, 22, 33, 44]);

    let zero = pixel_image([2, 2], &[], &[0, 0x08, 0, 0, 1, 0]);
    let frame = hg3::decode_frame(&zero, 0x8000_0001u32 as i32)
        .unwrap()
        .unwrap();
    assert_eq!(frame.info.size, [2, 2]);
    assert_eq!(frame.pixels, [0; 16]);
}

#[test]
fn native_pixels_reject_bad_runs_dimensions_streams_and_unsupported_encodings() {
    let mut cases = vec![
        pixel_image([2, 3], &LITERALS[..LITERALS.len() - 1], COMMANDS),
        pixel_image([2, 3], LITERALS, &[]),
        pixel_image([2, 3], LITERALS, &[0; 8]),
        pixel_image([2, 3], LITERALS, &[0x02]),
        pixel_image([1, 1], &[], &[0x08, 0x05]),
        pixel_image([1, 1], &[], &[0x28, 0x05]),
        pixel_image([1, 1], &[], &[0, 0, 0, 0x10, 0, 0, 0]),
        pixel_image([3, 3], LITERALS, COMMANDS),
        pixel_image([0, 3], &[], COMMANDS),
        pixel_image([u32::MAX, u32::MAX], &[], COMMANDS),
    ];
    let good = pixel_image([2, 3], LITERALS, COMMANDS);
    for (offset, value) in [(44, 24), (92, 1), (96, 2), (104, u32::MAX)] {
        let mut invalid = good.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(invalid);
    }
    for name in [b"img0001\0", b"img_jpg\0", b"img_wbp\0"] {
        let mut invalid = good.clone();
        invalid[76..84].copy_from_slice(name);
        cases.push(invalid);
    }
    let mut invalid = good;
    invalid[116] ^= 0xff;
    cases.push(invalid);
    for (index, bytes) in cases.iter().enumerate() {
        assert!(
            hg3::decode_frame(bytes, 0x8000_0001u32 as i32).is_err(),
            "case {index}"
        );
    }
}
