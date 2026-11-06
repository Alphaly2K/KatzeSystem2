use super::{HostState, Interpreter, NativeReturn, strings, utf8};
use crate::{Error, Result, read};

fn display<T: Copy + Into<u32>>(format: &[T]) -> Result<Vec<T>> {
    let mut output = Vec::new();
    let mut position = 0;
    while let Some(&value) = format.get(position) {
        position += 1;
        if value.into() == u32::from(b'%') {
            let Some(&next) = format.get(position) else {
                break;
            };
            position += 1;
            // 参数转换没有可用的可变参数，带标志的格式也保持诊断。
            if let Ok(byte) = u8::try_from(next.into()) {
                if b"-+ #*.0123456789hIlwcCdieEfFgGaAnopsSuUxXZ".contains(&byte) {
                    return Err(Error::new(position - 2, "调试文字的二次格式控制尚未支持"));
                }
            }
            output.push(next);
        } else {
            output.push(value);
        }
    }
    Ok(output)
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    address: u32,
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let source = strings::format(vm, address, 0)?;
    let source = &source[..source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len())];
    if source.len() > 2046 {
        return Err(Error::new(vm.pc, "调试文字超过基线转义缓冲区范围"));
    }
    let mut escaped = Vec::new();
    for (position, &byte) in source.iter().enumerate() {
        escaped.push(byte);
        if byte == b'%' && source.get(position + 1) != Some(&b'%') {
            escaped.push(byte);
        }
    }
    if escaped.len() > 2046 {
        return Err(Error::new(vm.pc, "调试文字超过基线转义缓冲区范围"));
    }
    if index == 1 {
        if host.debug_messages.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "调试文字队列超过上限"));
        }
        let mut output = display(&escaped)?;
        output.truncate(499);
        host.debug_messages.push(output);
    } else {
        if host.unicode_debug_messages.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "Unicode 调试文字队列超过上限"));
        }
        let mut output = display(&utf8::decode_units(&escaped, false)?)?;
        if output.len() >= 500 {
            return Err(Error::new(vm.pc, "Unicode 调试文字触及基线未终止缓冲区"));
        }
        output.push(u16::from(b'\n'));
        host.unicode_debug_messages.push(output);
    }
    Ok(NativeReturn::Continue)
}
