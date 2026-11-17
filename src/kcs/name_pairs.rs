use super::{HostState, Interpreter, NativeReturn, strings};
use crate::{Error, Result, read};

const NAME_BYTES: usize = 32;
const ROW_BYTES: usize = NAME_BYTES + 8;

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let address = read::u32_at(arguments, 0)?;
    let expected = if index == 289 { 12 } else { 16 };
    if arguments.len() != expected && !(index != 289 && address == 0 && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if index == 289 {
        let length = address
            .wrapping_mul(read::u32_at(arguments, 4)?)
            .wrapping_mul(read::u32_at(arguments, 8)?)
            .wrapping_mul(ROW_BYTES as u32);
        let length = read::sized(length, vm.pc)?;
        return Ok(NativeReturn::Value(
            host.insert_raw_buffer(vec![0; length], vm.pc)?,
        ));
    }
    if address == 0 {
        return Ok(NativeReturn::Continue);
    }

    let a = read::u32_at(arguments, 4)?;
    let b = read::u32_at(arguments, 8)?;
    let c = read::u32_at(arguments, 12)?;
    let name = if matches!(index, 291 | 292) {
        let name = strings::format(vm, a, 0)?;
        if name.len() >= NAME_BYTES {
            return Err(Error::new(vm.pc, "名称对照表的名称超过字段容量"));
        }
        name
    } else {
        Vec::new()
    };
    let mut rows = host.raw_buffer_rest(address)?.chunks_exact(ROW_BYTES);
    if index == 291 {
        let Some(slot) = rows.position(|row| row[0] == 0) else {
            return Err(Error::new(vm.pc, "名称对照表没有空槽"));
        };
        let slot = address + (slot * ROW_BYTES) as u32;
        let row = host.raw_buffer_mut(slot, ROW_BYTES)?;
        row[..name.len()].copy_from_slice(&name);
        row[name.len()] = 0;
        row[NAME_BYTES..NAME_BYTES + 4].copy_from_slice(&b.to_le_bytes());
        row[NAME_BYTES + 4..].copy_from_slice(&c.to_le_bytes());
        return Ok(NativeReturn::Continue);
    }

    for row in rows.take_while(|row| row[0] != 0) {
        if index == 292 {
            if read::zstr(&row[..NAME_BYTES], 0)?.eq_ignore_ascii_case(&name) {
                vm.write(b, &row[NAME_BYTES..NAME_BYTES + 4])?;
                vm.write(c, &row[NAME_BYTES + 4..])?;
                return Ok(NativeReturn::Value(1));
            }
        } else if read::u32_at(row, NAME_BYTES)? == a && read::u32_at(row, NAME_BYTES + 4)? == b {
            let length = read::zstr(&row[..NAME_BYTES], 0)?.len();
            vm.write(c, &row[..=length])?;
            return Ok(NativeReturn::Value(1));
        }
    }
    Ok(NativeReturn::Value(0))
}
