use katze_system2::hg3;
use katze_system2::{Result, host::storage::ReadSource};
use std::io::Write;
use std::{
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

struct ShortSource<'a> {
    bytes: &'a [u8],
    forbidden: Vec<Range<usize>>,
    read: AtomicUsize,
}

impl<'a> ShortSource<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            forbidden: Vec::new(),
            read: AtomicUsize::new(0),
        }
    }
}

impl ReadSource for ShortSource<'_> {
    fn len(&self) -> Result<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        let offset = offset as usize;
        let count = output
            .len()
            .min(3)
            .min(self.bytes.len().saturating_sub(offset));
        let end = offset + count;
        assert!(
            self.forbidden
                .iter()
                .all(|range| end <= range.start || offset >= range.end)
        );
        output[..count].copy_from_slice(&self.bytes[offset..end]);
        self.read.fetch_add(count, Ordering::Relaxed);
        Ok(count)
    }
}

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
        let expected = hg3::Metadata::parse(&valid[..end]).unwrap_err();
        assert_eq!(
            hg3::Metadata::read(&ShortSource::new(&valid[..end])).unwrap_err(),
            expected
        );
    }
    for (at, value) in [(12, 7u32), (12, u32::MAX), (28, 16), (32, 89), (56, 39)] {
        let mut invalid = valid.clone();
        invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(hg3::Metadata::parse(&invalid).is_err(), "{at} {value}");
        assert_eq!(
            hg3::Metadata::read(&ShortSource::new(&invalid)),
            hg3::Metadata::parse(&invalid)
        );
    }
}

const LITERALS: &[u8] = &[
    0x50, 0x55, 0x5a, 0x59, 0xaa, 0xf0, 0x49, 0x55, 0xb1, 0x22, 0xaa, 0xb0, 0x8d, 0xaa, 0x1b, 0xd3,
    0xff, 0xcf, 0x22, 0xaf, 0xff, 0x50,
];
const COMMANDS: &[u8] = &[0x61, 0x40, 0x96, 0x01];

fn compress(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn pixel_block(start: u32, rows: u32, literals: &[u8], commands: &[u8]) -> Vec<u8> {
    let data = compress(literals);
    let control = compress(commands);
    let mut out = Vec::new();
    for value in [
        start,
        rows,
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

fn pixel_image(size: [u32; 2], literals: &[u8], commands: &[u8]) -> Vec<u8> {
    let block = pixel_block(0, size[1], literals, commands);
    tagged_frame(size, 32, &[(b"img0000\0", &block)])
}

fn tagged_frame(size: [u32; 2], depth: u32, tags: &[(&[u8; 8], &[u8])]) -> Vec<u8> {
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
        depth,
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
    for (i, &(name, payload)) in tags.iter().enumerate() {
        out.extend(name);
        let len = payload.len() as u32;
        let next = if i + 1 == tags.len() { 0 } else { 16 + len };
        out.extend(next.to_le_bytes());
        out.extend(len.to_le_bytes());
        out.extend(payload);
    }
    out
}

#[test]
fn jpeg_uses_the_color_tag_before_native_and_reverses_valid_alpha_with_color() {
    let mut color = [32; 16];
    color[8..].fill(160);
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 100)
        .encode(&color, 1, 16, jpeg_encoder::ColorType::Luma)
        .unwrap();
    let packed = compress(&(0u8..20).collect::<Vec<_>>());
    let mut alpha = (packed.len() as u32).to_le_bytes().to_vec();
    alpha.extend(20u32.to_le_bytes());
    alpha.extend(packed);
    let id = 0x8000_0001u32 as i32;
    for (depth, declared) in [(32, 20u32), (24, 20), (32, 21)] {
        alpha[4..8].copy_from_slice(&declared.to_le_bytes());
        let source = tagged_frame(
            [1, 16],
            depth,
            &[
                (b"img0000\0", &[]),
                (b"img_al\0\0", &alpha),
                (b"img_jpg\0", &jpeg),
            ],
        );
        let frame = hg3::decode_frame(&source, id).unwrap().unwrap();
        let expected: Vec<_> = color
            .into_iter()
            .enumerate()
            .rev()
            .flat_map(|(i, c)| {
                [
                    c,
                    c,
                    c,
                    if (depth, declared) == (32, 20) {
                        i as u8
                    } else {
                        255
                    },
                ]
            })
            .collect();
        assert_eq!(frame.pixels, expected);
    }
}

#[test]
fn webp_takes_precedence_and_mode_overrides_the_jpeg_swap_hint() {
    let mut webp = Vec::new();
    image_webp::WebPEncoder::new(&mut webp)
        .encode(
            &[11, 22, 33, 44, 55, 66, 77, 88],
            1,
            2,
            image_webp::ColorType::Rgba8,
        )
        .unwrap();
    for (depth, mode, expected) in [
        (32, None, [77, 66, 55, 88, 33, 22, 11, 44]),
        (32, Some(0u32), [55, 66, 77, 88, 11, 22, 33, 44]),
        (24, Some(1), [66, 55, 44, 255, 33, 22, 11, 255]),
    ] {
        let mode = mode.map(u32::to_le_bytes);
        let mut tags: Vec<(&[u8; 8], &[u8])> = vec![
            (b"img_jpg\0", &[0xff, 0xd8]),
            (b"img_wbp\0", &webp),
            (b"img_al\0\0", &[0; 8]),
        ];
        if let Some(mode) = &mode {
            tags.push((b"imgmode\0", mode));
        }
        let bytes = tagged_frame([1, 2], depth, &tags);
        let mut source = ShortSource::new(&bytes);
        source
            .forbidden
            .extend([92..94, 126 + webp.len()..134 + webp.len()]);
        assert_eq!(
            hg3::decode_frame_from(&source, None)
                .unwrap()
                .unwrap()
                .pixels,
            expected
        );
    }
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
fn three_channel_slices_use_number_order_and_skip_unused_payloads() {
    let first = pixel_block(1, 2, &[120, 5, 113, 5, 41, 0, 162, 10], &[17, 8]);
    let second = pixel_block(0, 2, &[120, 5, 102, 5, 63, 0, 184, 10], &[17, 8]);
    let mut bytes = pixel_image([1, 3], &[], &[]);
    bytes.truncate(76);
    let stored_depth = 0x0005_0018u32;
    bytes[44..48].copy_from_slice(&stored_depth.to_le_bytes());
    let mut forbidden = Vec::new();
    for (index, (name, block)) in [
        (b"img0001\0", second.as_slice()),
        (b"img0000\0", first.as_slice()),
        (b"img0001\0", &[0; 24]),
        (b"img0003\0", &[0; 24]),
    ]
    .into_iter()
    .enumerate()
    {
        bytes.extend(name);
        let length = block.len() as u32;
        let next = if index == 3 { 0 } else { length + 16 };
        bytes.extend(next.to_le_bytes());
        bytes.extend(length.to_le_bytes());
        if index >= 2 {
            forbidden.push(bytes.len()..bytes.len() + block.len());
        }
        bytes.extend(block);
    }
    let mut source = ShortSource::new(&bytes);
    source.forbidden = forbidden;
    let id = 0x8000_0001u32 as i32;
    assert_eq!(
        hg3::Metadata::read(&source).unwrap().frames[0]
            .info
            .unwrap()
            .bit_depth,
        stored_depth
    );
    let decoded = hg3::decode_frame_from(&source, Some(id)).unwrap().unwrap();
    assert_eq!(decoded.info.bit_depth, 32);
    assert_eq!(
        decoded.pixels,
        [22, 79, 136, 255, 63, 120, 177, 255, 52, 109, 166, 255]
    );

    bytes[40..44].copy_from_slice(&4u32.to_le_bytes());
    let error = hg3::decode_frame(&bytes, id).unwrap_err();
    assert_eq!(error.message, "HG-3 原生像素数据未覆盖全部行");
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
    assert_eq!(
        hg3::Metadata::read(&ShortSource::new(&continued)).unwrap(),
        metadata
    );
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
    for (offset, value) in [(44, 16), (92, 1), (96, 2), (104, u32::MAX)] {
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
        assert_eq!(
            hg3::decode_frame_from(&ShortSource::new(bytes), Some(0x8000_0001u32 as i32)),
            hg3::decode_frame(bytes, 0x8000_0001u32 as i32),
            "case {index}"
        );
    }
}

#[test]
fn selective_reads_skip_unselected_pixels_and_keep_unsigned_and_duplicate_selection() {
    let mut bytes = pixel_image([2, 3], LITERALS, COMMANDS)[..12].to_vec();
    let mut payloads = Vec::new();
    let mut padding = Vec::new();
    for (index, id) in [9u32, 2, 2, u32::MAX].into_iter().enumerate() {
        let start = bytes.len();
        let mut frame = pixel_image([2, 3], LITERALS, COMMANDS);
        let image_end = frame.len();
        frame[16..20].copy_from_slice(&id.to_le_bytes());
        frame[48..52].copy_from_slice(&(index as u32 * 10).to_le_bytes());
        frame[84..88].copy_from_slice(&((image_end - 76) as u32).to_le_bytes());
        frame.extend(b"unknown\0");
        frame.extend(0u32.to_le_bytes());
        frame.extend(8192u32.to_le_bytes());
        frame.extend([0xff; 8192]);
        if index != 3 {
            let length = frame.len() - 12;
            frame[12..16].copy_from_slice(&(length as u32).to_le_bytes());
        }
        payloads.push(start + 80..start + image_end - 12);
        padding.push(start + image_end + 4..start + frame.len() - 12);
        bytes.extend(&frame[12..]);
    }
    let mut metadata_source = ShortSource::new(&bytes);
    metadata_source.forbidden = padding.iter().chain(&payloads).cloned().collect();
    let metadata = hg3::Metadata::read(&metadata_source).unwrap();
    let directory_bytes = metadata_source.read.load(Ordering::Relaxed);
    for (id, selected) in [
        (None, Some(1)),
        (Some(2), Some(1)),
        (Some(9), Some(0)),
        (Some(-1), Some(3)),
        (Some(99), None),
    ] {
        let mut source = ShortSource::new(&bytes);
        source.forbidden = padding.clone();
        source.forbidden.extend(
            payloads
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != selected)
                .map(|(_, range)| range.clone()),
        );
        let actual = hg3::decode_frame_from(&source, id).unwrap();
        let expected = hg3::decode_frame(&bytes, id.unwrap_or(2)).unwrap();
        assert_eq!(actual, expected);
        if let Some(selected) = selected {
            assert_eq!(actual.unwrap().info.offset[0], selected as i32 * 10);
        }
        assert_eq!(
            source.read.load(Ordering::Relaxed),
            directory_bytes + selected.map_or(0, |i| payloads[i].len())
        );
    }
    let last = metadata.frames[3].offset;
    let tag = payloads[3].start - 16;
    let mut unsupported = bytes.clone();
    unsupported[tag..tag + 8].copy_from_slice(b"img_jpa\0");
    assert_eq!(hg3::Metadata::parse(&unsupported).unwrap(), metadata);
    assert_eq!(
        hg3::decode_frame(&unsupported, 2),
        hg3::decode_frame(&bytes, 2)
    );
    assert_eq!(hg3::decode_frame(&unsupported, -1).unwrap_err().offset, tag);
    for (at, value, offset) in [
        (last, 7u32, last),
        (last + 20, 39, 0),
        (last + 76, u32::MAX, last + 76),
    ] {
        let mut invalid = bytes.clone();
        invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
        let expected = hg3::Metadata::parse(&invalid).unwrap_err();
        assert_eq!(expected.offset, offset);
        assert_eq!(
            hg3::decode_frame_from(&ShortSource::new(&invalid), Some(2)).unwrap_err(),
            expected
        );
    }
}

#[test]
fn source_length_limits_and_read_failures_report_diagnostics() {
    struct Declared<'a>(&'a [u8], u64);
    impl ReadSource for Declared<'_> {
        fn len(&self) -> Result<u64> {
            Ok(self.1)
        }
        fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
            self.0.read_at(offset, output)
        }
    }
    let bytes = image();
    for length in [64 * 1024 * 1024 + 1, u64::MAX] {
        let error = hg3::Metadata::read(&Declared(&[], length)).unwrap_err();
        assert_eq!(
            (error.offset, error.message.as_str()),
            (0, "无效 HG-3 容器")
        );
    }
    let error = hg3::Metadata::read(&Declared(&bytes[..10], bytes.len() as u64)).unwrap_err();
    assert_eq!(
        (error.offset, error.message.as_str()),
        (10, "读取源提前结束")
    );
}
