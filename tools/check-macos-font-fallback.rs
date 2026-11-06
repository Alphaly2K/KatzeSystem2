use katze_system2::message::{FontRequest, FontServices, TextEncoding, macos::MacFonts};

fn main() -> katze_system2::Result<()> {
    let mut fonts = MacFonts::new("");
    let mut count = 0;
    for encoding in [TextEncoding::Cp932, TextEncoding::Unicode] {
        for weight in [400, 700] {
            for italic in [false, true] {
                for size in [0, 12, 20, 28, 32] {
                    let request = FontRequest {
                        encoding,
                        name: Vec::new(),
                        weight,
                        italic,
                        backend: 0,
                        glyph_parameter: 0,
                        measure_parameter: 0,
                    };
                    fonts.validate(&request)?;
                    let missing = fonts.rasterize(&request, '\u{378}', size)?;
                    for character in [
                        'A', 'B', ' ', '【', '】', 'あ', '一', '…', '\u{ffff}', '\u{378}',
                    ] {
                        let width = fonts.measure(&request, character, size)?;
                        let glyph = fonts.rasterize(&request, character, size)?;
                        assert_eq!(glyph.advance(), width);
                        assert!(glyph.coverage().iter().all(|&value| value <= 64));
                        if size == 0 || character == ' ' {
                            assert!(glyph.coverage().is_empty());
                        } else {
                            assert!(width > 0);
                            if matches!(character, '【' | '】' | 'あ' | '一') {
                                assert_ne!(glyph, missing, "CJK glyph must differ from .notdef");
                                assert!(glyph.coverage().iter().any(|&value| value != 0));
                            }
                        }
                        count += 1;
                    }
                }
            }
        }
    }
    println!("PASS macOS default-font cascade: {count} measurement/rasterization pairs");
    Ok(())
}
