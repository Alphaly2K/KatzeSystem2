use super::{HostState, Interpreter, NativeReturn};
use crate::{Error, Result, read};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
) -> Result<NativeReturn> {
    if arguments.len() != 8 {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    let handle = read::u32_at(arguments, 0)?;
    let address = read::u32_at(arguments, 4)?;
    if handle == 0 || address == 0 {
        return Ok(NativeReturn::Continue);
    }
    if index == 900 {
        let entries = host
            .integer_maps
            .get(&handle)
            .ok_or_else(|| Error::new(vm.pc, "无效的 KCS 整数字典句柄"))?;
        let length = entries.keys().try_fold(4usize, |length, key| {
            length
                .checked_add((key.len() + 4) & !3)
                .and_then(|n| n.checked_add(4))
                .filter(|&n| n <= read::MAX_BYTES)
                .ok_or_else(|| Error::new(vm.pc, "KCS 字典序列化超过上限"))
        })?;
        let entries: Vec<_> = entries
            .iter()
            .map(|(key, value)| (key.clone(), *value))
            .collect();
        let bytes = host.raw_buffer_mut(address, length)?;
        bytes[..4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        let mut offset = 4;
        for (key, value) in entries {
            bytes[offset..offset + key.len()].copy_from_slice(&key);
            bytes[offset + key.len()] = 0;
            offset += (key.len() + 4) & !3;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            offset += 4;
        }
    } else {
        let bytes = host.raw_buffer_rest(address)?.to_vec();
        let count = read::u32_at(&bytes, 0)? as usize;
        if count > read::MAX_ITEMS {
            return Err(Error::new(0, "KCS 字典导入数量超过上限"));
        }
        if count == 0 {
            return Ok(NativeReturn::Continue);
        }
        let entries = host
            .integer_maps
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "无效的 KCS 整数字典句柄"))?;
        let mut offset = 4;
        for _ in 0..count {
            let key = read::zstr(&bytes, offset)?;
            offset = offset
                .checked_add((key.len() + 4) & !3)
                .ok_or_else(|| Error::new(offset, "KCS 字典导入偏移溢出"))?;
            let value = read::u32_at(&bytes, offset)?;
            offset += 4;
            if entries.len() == read::MAX_ITEMS && !entries.contains_key(key) {
                return Err(Error::new(offset, "KCS 整数字典条目超过上限"));
            }
            entries.entry(key.to_vec()).or_insert(value);
        }
    }
    Ok(NativeReturn::Continue)
}
