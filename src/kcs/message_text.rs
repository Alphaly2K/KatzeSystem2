use super::{Interpreter, NativeReturn, strings};
use crate::{Result, message::TextEncoding};

fn characters(source: &[u8], encoding: TextEncoding) -> Result<String> {
    if encoding == TextEncoding::Unicode {
        return Ok(super::utf8::decode_units(source, true)?
            .into_iter()
            .map(|unit| char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}'))
            .collect());
    }
    let mut text = String::new();
    let mut position = 0;
    while let Some(&byte) = source.get(position) {
        let width = strings::character_width(&source[position..]);
        let code = if byte == 0xff {
            0xffff
        } else if width == 2 {
            u32::from(u16::from_be_bytes([byte, source[position + 1]]))
        } else {
            u32::from(byte)
        };
        text.push(char::from_u32(code).unwrap());
        position += width;
    }
    Ok(text)
}

pub(super) fn call(index: u32, args: &[u32], vm: &mut Interpreter) -> Result<NativeReturn> {
    let source = strings::format(vm, args[1], 0)?;
    if vm.read(args[0], 1).is_err() {
        return Ok(NativeReturn::Value(0));
    }
    let source = &source[..source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len())];
    let encoding = if index == 392 {
        TextEncoding::Cp932
    } else {
        TextEncoding::Unicode
    };
    let text = characters(source, encoding)?;
    let codes = crate::message::extract_codes(&text, encoding)?;
    let mut output = Vec::new();
    for code in codes {
        if encoding == TextEncoding::Cp932 {
            if code > 0xff {
                output.push((code >> 8) as u8);
            }
            output.push(code as u8);
        } else {
            let mut buffer = [0; 4];
            output.extend(
                char::from_u32(u32::from(code))
                    .unwrap()
                    .encode_utf8(&mut buffer)
                    .as_bytes(),
            );
        }
    }
    let length = output
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(output.len());
    output.push(0);
    vm.write(args[0], &output)?;
    Ok(NativeReturn::Value(length as u32))
}
