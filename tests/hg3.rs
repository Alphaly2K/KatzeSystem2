use katze_system2::hg3;

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
