use super::{Interpreter, NativeReturn};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

pub(super) fn decode_units(source: &[u8], terminal_sentinel: bool) -> Result<Vec<u16>> {
    let mut units = Vec::new();
    let mut position = 0;
    while let Some(&byte) = source.get(position) {
        if byte == 0 {
            break;
        }
        // 消息入口还将末尾单个 FF 作为哨兵；普通转换只识别 FF FF。
        if byte == 0xff
            && (source.get(position + 1) == Some(&0xff)
                || terminal_sentinel && position + 1 == source.len())
        {
            units.push(0xffff);
            position += (source.len() - position).min(2);
            continue;
        }
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
            .ok_or_else(|| Error::new(position, "UTF-8 字符超出输入范围"))?;
        let character = String::from_utf8_lossy(bytes).chars().next().unwrap();
        // 基线逐字符转换后只取第一个 UTF-16 单元，包括非 BMP 字符的高代理单元。
        units.push(character.encode_utf16(&mut [0; 2])[0]);
        position += width;
    }
    Ok(units)
}

fn byte(vm: &Interpreter, address: u32) -> Result<u8> {
    Ok(vm.read(address, 1)?[0])
}

// 基线只检查末字节之前的续字节；负返回值仍参与截断偏移计算。
fn width(vm: &Interpreter, address: u32) -> Result<i32> {
    let leading = byte(vm, address)?.leading_ones();
    match leading {
        0 | 6.. => Ok(1),
        1 => Ok(-2),
        _ => {
            for index in 1..leading - 1 {
                if byte(vm, address.wrapping_add(index))? & 0xc0 != 0x80 {
                    return Ok(-3);
                }
            }
            Ok(leading as i32)
        }
    }
}

fn cp932(source: &[u8]) -> Vec<u8> {
    let mut result = String::new();
    let mut position = 0;
    while let Some(&value) = source.get(position) {
        match value {
            0..=0x80 => result.push(char::from(value)),
            0xa0 => result.push('\u{f8f0}'),
            0xa1..=0xdf => result.push(char::from_u32(0xff61 + u32::from(value - 0xa1)).unwrap()),
            0xfd..=0xff => result.push(char::from_u32(0xf8f1 + u32::from(value - 0xfd)).unwrap()),
            _ => {
                let end = (position + 2).min(source.len());
                if let Some(decoded) = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&source[position..end])
                {
                    result.push_str(&decoded);
                } else {
                    result.push('\u{30fb}');
                }
                position = end;
                continue;
            }
        }
        position += 1;
    }
    result.into_bytes()
}

fn truncate(vm: &mut Interpreter, address: u32, limit: u32, bytes: bool) -> Result<NativeReturn> {
    let mut position = 0u32;
    let mut count = 0u32;
    let mut visited = BTreeMap::new();
    while (bytes || count < limit) && byte(vm, address.wrapping_add(position))? != 0 {
        if let Some(previous) = visited.insert(position, count) {
            if bytes {
                return Err(Error::new(vm.pc, "UTF-8 字节截断进入重复位置"));
            }
            let period = count - previous;
            // 负步长形成的环可按整圈跳过，保持相同的最终地址。
            count += (limit - count) / period * period;
            if count == limit {
                break;
            }
        }
        if visited.len() > read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "UTF-8 截断超过上限"));
        }
        let next = position.wrapping_add(width(vm, address.wrapping_add(position))? as u32);
        if bytes && next >= limit {
            break;
        }
        position = next;
        count += 1;
    }
    vm.write(address.wrapping_add(position), &[0; 2])?;
    Ok(NativeReturn::Value(position))
}

pub(super) fn call(index: u32, args: &[u32], vm: &mut Interpreter) -> Result<NativeReturn> {
    match index {
        1045 | 1046 => truncate(vm, args[0], args[1], index == 1045),
        1051 => {
            let needle = vm.read_string(args[1])?;
            let needle = if needle.len() > 4000 {
                Vec::new()
            } else {
                let value = cp932(&needle);
                if std::str::from_utf8(&value).unwrap().encode_utf16().count() >= 260 {
                    return Err(Error::new(vm.pc, "UTF-8 查询串超过基线转换缓冲范围"));
                }
                value
            };
            let length = vm.read_string(args[0])?.len() as u32;
            let mut position = 0u32;
            while byte(vm, args[0].wrapping_add(position))? != 0 {
                let mut matches = true;
                for (index, &value) in needle.iter().enumerate() {
                    if byte(
                        vm,
                        args[0].wrapping_add(position).wrapping_add(index as u32),
                    )? != value
                    {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    return Ok(NativeReturn::Value(position));
                }
                let width = width(vm, args[0].wrapping_add(position))?;
                if width <= 0 {
                    break;
                }
                position = position.wrapping_add(width as u32);
                if position as usize > read::MAX_BYTES {
                    return Err(Error::new(vm.pc, "UTF-8 查找超过上限"));
                }
            }
            Ok(NativeReturn::Value(length))
        }
        1063 => {
            let mut value = cp932(&vm.read_string(args[1])?);
            if value.len() < 256 {
                value.push(0);
                vm.write(args[0], &value)?;
            }
            Ok(NativeReturn::Continue)
        }
        _ => unreachable!(),
    }
}
