#![cfg(target_os = "macos")]

use katze_system2::message::{FontRequest, FontServices, TextEncoding, macos::MacFonts};

fn request() -> FontRequest {
    FontRequest {
        encoding: TextEncoding::Unicode,
        name: "Katze Synthetic Triangle".encode_utf16().collect(),
        weight: 400,
        backend: 0,
        glyph_parameter: 1,
        measure_parameter: 0,
    }
}

#[test]
fn private_native_font_keeps_selection_measurement_and_rasterization_consistent() {
    let mut fonts = MacFonts::new("");
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
    invalid.glyph_parameter = 2;
    assert!(fonts.validate(&invalid).is_err());
    invalid = request.clone();
    invalid.name = vec![0xd800];
    assert!(fonts.validate(&invalid).is_err());
    invalid = request.clone();
    invalid.name = "@Katze Synthetic Triangle".encode_utf16().collect();
    assert!(fonts.validate(&invalid).is_err());
    assert!(fonts.enumerate(128, 1).is_err());
}
