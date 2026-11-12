use super::{words, write_string};
use crate::kcs::host::TextRecord;
use crate::kcs::{HostState, Interpreter, NativeReturn, strings};
use crate::{Error, Result, expression, read};
use std::collections::BTreeMap;

pub(super) fn call(
    state: &mut expression::State,
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if matches!(index, 243 | 253) {
        let handle = read::u32_at(arguments, 0)?;
        if handle == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if index == 253 {
            let Some(bytes) = host
                .data_container(handle)?
                .record(read::u32_at(arguments, 4)?)?
            else {
                return Ok(NativeReturn::Value(0));
            };
            let count = read::u32_at(bytes, 0)? as usize;
            if count > read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "CSS 字符串数量超过读取上限"));
            }
            let mut strings = BTreeMap::new();
            let mut offset = 4;
            for _ in 0..count {
                let id = read::u32_at(bytes, offset)?;
                offset += 4;
                let text = read::zstr(bytes, offset)?;
                strings.entry(id).or_insert_with(|| text.to_vec());
                offset += text.len() + 1;
            }
            state.strings = strings;
            return Ok(NativeReturn::Value(1));
        }
        let mut bytes = (state.strings.len() as u32).to_le_bytes().to_vec();
        for (id, text) in &state.strings {
            let length = text.len();
            let text = text.split(|&byte| byte == 0).next().unwrap();
            if bytes
                .len()
                .checked_add(length + 13)
                .is_none_or(|size| size > read::MAX_BYTES)
            {
                return Err(Error::new(vm.pc, "CSS 字符串记录超过读取上限"));
            }
            bytes.extend(id.to_le_bytes());
            let end = bytes.len() + length + 1;
            bytes.extend(text);
            bytes.resize(end, 0);
        }
        bytes.extend([0; 8]);
        host.data_container_mut(handle)?
            .append(read::u32_at(arguments, 4)?, &bytes)?;
        return Ok(NativeReturn::Value(1));
    }
    if index == 869 {
        if arguments.len() != 16 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut record = TextRecord::default();
        for (position, bytes) in arguments.chunks_exact(4).enumerate() {
            record.strings[position] =
                strings::format(vm, u32::from_le_bytes(bytes.try_into().unwrap()), 0)?;
        }
        let handle = host.handle_allocator().allocate()?;
        host.text_records.insert(handle, record);
        return Ok(NativeReturn::Value(handle));
    }
    if index == 873 {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let handle = read::u32_at(arguments, 0)?;
        let address = read::u32_at(arguments, 4)?;
        let data = host.raw_buffer_rest(address)?.to_vec();
        host.text_records
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "KCS 文本记录句柄无效"))?
            .payload = data;
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 870 | 872 | 874 | 875) {
        let count = match index {
            870 => 1,
            872 | 874 => 2,
            _ => 3,
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let handle = read::u32_at(arguments, 0)?;
        if let Some(record) = host.text_records.get_mut(&handle) {
            record.calls.push((index, arguments.to_vec()));
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 419 | 515) {
        if host.integer_maps.len() + host.sequence_maps.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "KCS 集合对象数量超过上限"));
        }
        let handle = host.handle_allocator().allocate_at(vm.pc)?;
        if index == 419 {
            host.sequence_maps.insert(handle, Default::default());
        } else {
            host.integer_maps.insert(handle, BTreeMap::new());
        }
        return Ok(NativeReturn::Value(handle));
    }
    let count = match index {
        420 | 421 | 516 | 898 | 899 => 1,
        422 | 423 | 518 | 897 | 1052 => 2,
        517 | 896 | 902 => 3,
        _ => unreachable!(),
    };
    let args = words(arguments, count, vm.pc)?;
    Ok(match index {
        420 => {
            if args[0] != 0 && host.sequence_maps.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS 序列对象句柄无效"));
            }
            NativeReturn::Continue
        }
        421..=423 => {
            if args[0] == 0 {
                return Ok(if index == 423 {
                    NativeReturn::Value(0)
                } else {
                    NativeReturn::Continue
                });
            }
            let map = host
                .sequence_maps
                .get_mut(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "KCS 序列对象句柄无效"))?;
            if index == 421 {
                map.clear();
                NativeReturn::Continue
            } else {
                let source = strings::format(vm, args[1], 0)?;
                if index == 422 {
                    map.register(&source, host.sequence_name_mode)?;
                    NativeReturn::Continue
                } else {
                    NativeReturn::Value(u32::from(map.contains(&source)?))
                }
            }
        }
        516 => {
            if args[0] != 0 && host.integer_maps.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "无效的 KCS 整数字典句柄"));
            }
            NativeReturn::Continue
        }
        517 | 518 => {
            if args[0] == 0 {
                return Ok(if index == 517 {
                    NativeReturn::Continue
                } else {
                    NativeReturn::Value(u32::MAX)
                });
            }
            let entries = host
                .integer_maps
                .get_mut(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "无效的 KCS 整数字典句柄"))?;
            let key = vm.read_string(args[1])?;
            if index == 518 {
                NativeReturn::Value(entries.get(key.as_ref()).copied().unwrap_or(u32::MAX))
            } else {
                if entries.len() >= read::MAX_ITEMS && !entries.contains_key(key.as_ref()) {
                    return Err(Error::new(vm.pc, "KCS 整数字典条目超过上限"));
                }
                entries.entry(key.to_vec()).or_insert(args[2]);
                NativeReturn::Continue
            }
        }
        896..=899 | 902 | 1052 => {
            if (index == 902 && vm.read(args[2], 1).is_err()) || args[0] == 0 {
                return Ok(if matches!(index, 897 | 899 | 902 | 1052) {
                    NativeReturn::Value(0)
                } else {
                    NativeReturn::Continue
                });
            }
            let entries = host
                .integer_maps
                .get_mut(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "无效的 KCS 整数字典句柄"))?;
            match index {
                1052 => {
                    let text = strings::format(vm, args[1], 0)?;
                    let text = text.split(|&byte| byte == 0).next().unwrap();
                    NativeReturn::Value(
                        entries
                            .iter()
                            .find_map(|(key, value)| {
                                text.get(..key.len())
                                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(key))
                                    .then_some(*value)
                            })
                            .unwrap_or(0),
                    )
                }
                896 | 897 => {
                    let key = strings::format(vm, args[1], 0)?;
                    let key = key.split(|&byte| byte == 0).next().unwrap();
                    if index == 897 {
                        NativeReturn::Value(entries.get(key).copied().unwrap_or(0))
                    } else {
                        if entries.len() >= read::MAX_ITEMS && !entries.contains_key(key) {
                            return Err(Error::new(vm.pc, "KCS 整数字典条目超过上限"));
                        }
                        entries.insert(key.to_vec(), args[2]);
                        NativeReturn::Continue
                    }
                }
                898 => {
                    entries.clear();
                    NativeReturn::Continue
                }
                899 => {
                    let size = entries.keys().try_fold(4u32, |size, key| {
                        let length = u32::try_from(key.len()).ok()?.checked_add(4)? & !3;
                        size.checked_add(length)?.checked_add(4)
                    });
                    NativeReturn::Value(
                        size.ok_or_else(|| Error::new(vm.pc, "KCS 字典序列化大小溢出"))?,
                    )
                }
                902 => {
                    if let Some(key) = entries.keys().nth(args[1] as usize) {
                        write_string(vm, args[2], key, 1)?;
                        NativeReturn::Value(1)
                    } else {
                        NativeReturn::Value(0)
                    }
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    })
}
