use katze_system2::message::{FontRequest, FontServices, Fonts, TextEncoding};

fn main() -> katze_system2::Result<()> {
    let mut fonts = Fonts::new("");
    let mut count = 0;
    for glyph_parameter in [0, 128] {
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
                            glyph_parameter,
                            measure_parameter: 0,
                        };
                        fonts.validate(&request)?;
                        let missing = fonts.rasterize(&request, '\u{378}', size)?;
                        for character in [
                            'A', 'B', ' ', '【', '】', 'あ', '一', '…', '\u{ffff}', '\u{378}',
                            '变', '这',
                        ] {
                            let width = fonts.measure(&request, character, size)?;
                            let glyph = fonts.rasterize(&request, character, size)?;
                            assert_eq!(glyph.advance(), width);
                            assert!(glyph.coverage().iter().all(|&value| value <= 64));
                            if size == 0 || character == ' ' {
                                assert!(glyph.coverage().is_empty());
                            } else {
                                assert!(width > 0);
                                if matches!(character, '【' | '】' | 'あ' | '一' | '变' | '这')
                                {
                                    assert_ne!(
                                        glyph, missing,
                                        "CJK glyph must differ from .notdef"
                                    );
                                    assert!(glyph.coverage().iter().any(|&value| value != 0));
                                }
                            }
                            count += 1;
                        }
                        if size != 0 {
                            assert_ne!(
                                fonts.rasterize(&request, '变', size)?,
                                fonts.rasterize(&request, '这', size)?,
                                "distinct Han characters must not share a range placeholder"
                            );
                        }
                    }
                }
            }
        }
    }
    println!("PASS default-font cascade: {count} measurement/rasterization pairs");
    Ok(())
}
