use katze_system2::message::{FontRequest, FontServices, Fonts, TextEncoding};

fn request() -> FontRequest {
    FontRequest {
        encoding: TextEncoding::Unicode,
        name: "Katze Synthetic Triangle".encode_utf16().collect(),
        weight: 400,
        italic: false,
        backend: 0,
        glyph_parameter: 1,
        measure_parameter: 0,
    }
}

#[test]
fn private_font_keeps_selection_measurement_and_rasterization_consistent() {
    let mut fonts = Fonts::without_system_fonts("");
    let request = request();
    assert_eq!(
        fonts
            .add_data(include_bytes!("fixtures/synthetic-triangle.ttf"))
            .unwrap(),
        1
    );
    fonts.validate(&request).unwrap();
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
    assert_eq!(fonts.measure(&request, ' ', 20).unwrap(), 10);
    let glyph = fonts.rasterize(&request, 'A', 20).unwrap();
    assert_eq!(glyph.size(), [16, 16]);
    assert_eq!(glyph.offset(), [3, 1]);
    assert_eq!(glyph.advance(), 20);
    assert!(glyph.coverage().contains(&64));
    assert!(glyph.coverage().iter().any(|&v| v > 0 && v < 64));
    let top: u32 = glyph.coverage()[..16].iter().map(|&v| u32::from(v)).sum();
    let bottom: u32 = glyph.coverage()[240..].iter().map(|&v| u32::from(v)).sum();
    assert!(top < bottom, "bitmap rows must start at the top");
    assert!(
        fonts
            .rasterize(&request, ' ', 20)
            .unwrap()
            .coverage()
            .is_empty()
    );
    assert!(
        fonts
            .rasterize(&request, 'A', 0)
            .unwrap()
            .coverage()
            .is_empty()
    );
    assert!(fonts.rasterize(&request, 'A', u32::MAX).is_err());
    assert!(
        fonts
            .rasterize(&request, 'B', 20)
            .unwrap_err()
            .message
            .contains("U+0042")
    );
    assert!(fonts.add_data(b"invalid").is_err());
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);

    let mut invalid = request.clone();
    invalid.backend = 1;
    assert!(fonts.validate(&invalid).is_err());
    invalid = request.clone();
    invalid.glyph_parameter = 3;
    assert!(fonts.validate(&invalid).is_err());
    invalid = request.clone();
    invalid.name = vec![0xd800];
    assert!(fonts.validate(&invalid).is_err());
    invalid = request.clone();
    invalid.name = "@Katze Synthetic Triangle".encode_utf16().collect();
    assert!(fonts.validate(&invalid).is_err());
    assert!(fonts.enumerate(128, 1).unwrap().is_empty());
    assert_eq!(fonts.enumerate(u32::MAX, 1).unwrap(), [request.name]);
}

fn tables() -> std::collections::BTreeMap<[u8; 4], Vec<u8>> {
    let bytes = include_bytes!("fixtures/synthetic-triangle.ttf");
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    (0..count)
        .map(|index| {
            let record = &bytes[12 + index * 16..28 + index * 16];
            let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
            let length = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
            (
                record[..4].try_into().unwrap(),
                bytes[offset..offset + length].to_vec(),
            )
        })
        .collect()
}

fn font_file(tables: std::collections::BTreeMap<[u8; 4], Vec<u8>>) -> Vec<u8> {
    let mut bytes = vec![0; 12 + tables.len() * 16];
    bytes[..4].copy_from_slice(&0x10000u32.to_be_bytes());
    bytes[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    for (index, (tag, data)) in tables.into_iter().enumerate() {
        let offset = bytes.len() as u32;
        let record = &mut bytes[12 + index * 16..28 + index * 16];
        record[..4].copy_from_slice(&tag);
        record[8..12].copy_from_slice(&offset.to_be_bytes());
        record[12..16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend(data);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
    }
    bytes
}

fn collection(fonts: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"ttcf\0\x01\0\0".to_vec();
    bytes.extend((fonts.len() as u32).to_be_bytes());
    bytes.resize(12 + fonts.len() * 4, 0);
    for (index, font) in fonts.iter().enumerate() {
        let base = bytes.len() as u32;
        bytes[12 + index * 4..16 + index * 4].copy_from_slice(&base.to_be_bytes());
        let mut font = font.clone();
        let count = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
        for record in 0..count {
            let offset = &mut font[20 + record * 16..24 + record * 16];
            let value = u32::from_be_bytes(offset.try_into().unwrap());
            offset.copy_from_slice(&(value + base).to_be_bytes());
        }
        bytes.extend(font);
    }
    bytes
}

#[test]
fn collection_names_charset_enumeration_and_default_cascade_share_the_same_faces() {
    let original = font_file(tables());
    let mut fallback = tables();
    let name = fallback.get_mut(b"name").unwrap();
    let family: Vec<_> = "Katze Synthetic Fallback"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    assert_eq!(family.len(), name.len() - 18);
    name[18..].copy_from_slice(&family);
    let cmap = fallback.get_mut(b"cmap").unwrap();
    for offset in [28, 36] {
        cmap[offset..offset + 2].copy_from_slice(&66u16.to_be_bytes());
    }
    cmap[42..44].copy_from_slice(&(-64i16).to_be_bytes());
    fallback.get_mut(b"hhea").unwrap()[4..6].copy_from_slice(&600i16.to_be_bytes());
    fallback.get_mut(b"post").unwrap()[12..16].copy_from_slice(&1u32.to_be_bytes());
    let mut os2 = vec![0; 86];
    os2[..2].copy_from_slice(&1u16.to_be_bytes());
    os2[4..6].copy_from_slice(&400u16.to_be_bytes());
    os2[78..82].copy_from_slice(&(1u32 << 17).to_be_bytes());
    fallback.insert(*b"OS/2", os2);
    let bytes = collection(&[original, font_file(fallback)]);
    let mut fonts = Fonts::without_system_fonts("");
    let mut request = request();
    request.name.clear();
    request.glyph_parameter = 128;
    assert!(fonts.validate(&request).is_err());
    assert_eq!(fonts.add_data(&bytes).unwrap(), 2);
    fonts.validate(&request).unwrap();
    assert_eq!(fonts.measure(&request, 'B', 20).unwrap(), 20);
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
    assert_eq!(
        fonts.rasterize(&request, 'A', 20).unwrap().offset(),
        [3, -3]
    );
    assert_eq!(fonts.measure(&request, '\u{378}', 20).unwrap(), 10);
    for (charset, variable, expected) in [
        (
            u32::MAX,
            1,
            vec!["Katze Synthetic Triangle", "Katze Synthetic Fallback"],
        ),
        (1, 0, vec!["Katze Synthetic Fallback"]),
        (128, 1, vec!["Katze Synthetic Fallback"]),
        (134, 1, vec![]),
        (999, 1, vec![]),
    ] {
        assert_eq!(
            fonts.enumerate(charset, variable).unwrap(),
            expected
                .into_iter()
                .map(|name| name.encode_utf16().collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
    }
    request.name = "KATZE SYNTHETIC TRIANGLE".encode_utf16().collect();
    assert!(fonts.measure(&request, 'B', 20).is_err());
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
    assert!(fonts.add_data(b"invalid").is_err());
    assert_eq!(fonts.enumerate(1, 1).unwrap().len(), 2);
    assert_eq!(
        fonts
            .add_data(include_bytes!("fixtures/synthetic-triangle.ttf"))
            .unwrap(),
        1
    );
    assert_eq!(fonts.enumerate(1, 1).unwrap().len(), 3);
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
}

#[test]
fn missing_styles_are_synthesized_consistently_and_real_bold_faces_take_precedence() {
    let mut fonts = Fonts::without_system_fonts("");
    fonts
        .add_data(include_bytes!("fixtures/synthetic-triangle.ttf"))
        .unwrap();
    for (bold, italic, width, advance) in [
        (false, false, 16, 20),
        (true, false, 17, 21),
        (false, true, 19, 20),
        (true, true, 20, 21),
    ] {
        let mut request = request();
        request.weight = if bold { 700 } else { 400 };
        request.italic = italic;
        assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), advance);
        let glyph = fonts.rasterize(&request, 'A', 20).unwrap();
        assert_eq!(glyph.advance(), advance);
        assert_eq!(glyph.size(), [width, 16]);
        assert!(glyph.coverage().iter().all(|&value| value <= 64));
    }
    let mut bold = tables();
    let mut os2 = vec![0; 86];
    os2[..2].copy_from_slice(&1u16.to_be_bytes());
    os2[4..6].copy_from_slice(&700u16.to_be_bytes());
    bold.insert(*b"OS/2", os2);
    fonts.add_data(&font_file(bold)).unwrap();
    let mut request = request();
    request.weight = 700;
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
    assert_eq!(fonts.rasterize(&request, 'A', 20).unwrap().size(), [16, 16]);
}

#[test]
fn automatic_cascade_skips_last_resort_ranges_before_real_character_coverage() {
    let mut primary = tables();
    let cmap = primary.get_mut(b"cmap").unwrap();
    for offset in [28, 36] {
        cmap[offset..offset + 2].copy_from_slice(&66u16.to_be_bytes());
    }
    cmap[42..44].copy_from_slice(&(-64i16).to_be_bytes());
    let mut placeholder = tables();
    let flags = &mut placeholder.get_mut(b"head").unwrap()[16..18];
    let value = u16::from_be_bytes(flags.try_into().unwrap());
    flags.copy_from_slice(&(value | 0x4000).to_be_bytes());
    placeholder.get_mut(b"hmtx").unwrap()[8..10].copy_from_slice(&500u16.to_be_bytes());
    let name: Vec<_> = "Katze Synthetic LastFont"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    placeholder.get_mut(b"name").unwrap()[18..].copy_from_slice(&name);
    let bytes = collection(&[
        font_file(placeholder),
        font_file(primary),
        font_file(tables()),
    ]);
    let mut fonts = Fonts::without_system_fonts("");
    fonts.add_data(&bytes).unwrap();
    let mut request = request();
    request.name.clear();
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 20);
    assert_eq!(fonts.rasterize(&request, 'A', 20).unwrap().advance(), 20);
    assert_eq!(fonts.measure(&request, 'B', 20).unwrap(), 20);
    request.name = "Katze Synthetic LastFont".encode_utf16().collect();
    assert_eq!(fonts.measure(&request, 'A', 20).unwrap(), 10);
}
