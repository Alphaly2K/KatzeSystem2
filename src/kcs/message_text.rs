use super::{Interpreter, NativeReturn, strings};
use crate::{Error, Result, message::TextEncoding};

fn characters(source: &[u8], encoding: TextEncoding) -> Result<String> {
    let mut text = String::new();
    let mut position = 0;
    while let Some(&byte) = source.get(position) {
        let (code, width) = if encoding == TextEncoding::Cp932 {
            let width = strings::character_width(&source[position..]);
            let code = if byte == 0xff {
                0xffff
            } else if width == 2 {
                u32::from(u16::from_be_bytes([byte, source[position + 1]]))
            } else {
                u32::from(byte)
            };
            (code, width)
        } else if byte == 0xff && (position + 1 == source.len() || source[position + 1] == 0xff) {
            (0xffff, (source.len() - position).min(2))
        } else {
            let leading = byte.leading_ones();
            if leading == 1 {
                break;
            }
            let width = if leading >= 6 {
                1
            } else {
                leading.max(1) as usize
            };
            if (1..width.saturating_sub(1)).any(|index| {
                source
                    .get(position + index)
                    .is_none_or(|byte| byte & 0xc0 != 0x80)
            }) {
                break;
            }
            let bytes = source
                .get(position..position + width)
                .ok_or_else(|| Error::new(position, "UTF-8 消息字符超出输入范围"))?;
            let character = String::from_utf8_lossy(bytes).chars().next().unwrap();
            // 原入口只保留转换结果的第一个 UTF-16 单元，输出时独立代理单元成为替代字符。
            (
                if character.len_utf16() == 2 {
                    0xfffd
                } else {
                    character as u32
                },
                width,
            )
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
