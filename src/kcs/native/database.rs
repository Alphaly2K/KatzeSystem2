use super::words;
use crate::kcs::{HostState, Interpreter, NativeReturn, strings};
use crate::{Error, Result, read};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
) -> Result<NativeReturn> {
    if index == 869 {
        return Err(Error::new(vm.pc, "数据库装载需要资源服务"));
    }
    if index == 871 && arguments == [0; 4] {
        return Ok(NativeReturn::Continue);
    }
    let count = match index {
        870 | 878 => 1,
        871 => 3,
        _ => 2,
    };
    let args = words(arguments, count, vm.pc)?;
    let handle = args[0];
    if handle == 0 {
        return if matches!(index, 872 | 873) {
            Err(Error::new(vm.pc, "数据库零句柄分支未初始化返回值"))
        } else {
            Ok(NativeReturn::Continue)
        };
    }
    if index == 870 {
        host.databases
            .remove(&handle)
            .ok_or_else(|| Error::new(vm.pc, "数据库句柄无效"))?;
        return Ok(NativeReturn::Continue);
    }
    let database = host
        .databases
        .get_mut(&handle)
        .ok_or_else(|| Error::new(vm.pc, "数据库句柄无效"))?;
    match index {
        871 => {
            database.mark(&strings::format(vm, args[2], 0)?, args[1]);
            Ok(NativeReturn::Continue)
        }
        878 => {
            database.dirty = 0;
            Ok(NativeReturn::Continue)
        }
        872 => {
            let size = database.saved_size()?;
            if args[1] != 0 {
                let bytes = database.export()?;
                host.raw_buffer_mut(args[1], size)?.copy_from_slice(&bytes);
            }
            Ok(NativeReturn::Value(size as u32))
        }
        873 => {
            if args[1] == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let count = read::u32_at(host.raw_buffer(args[1], 4)?, 0)? as usize;
            let size = super::super::database::saved_size(count)?;
            let mut bytes = host.raw_buffer(args[1], size)?.to_vec();
            host.databases
                .get_mut(&handle)
                .unwrap()
                .import(&mut bytes)?;
            host.raw_buffer_mut(args[1], size)?.copy_from_slice(&bytes);
            Ok(NativeReturn::Value(1))
        }
        _ => unreachable!(),
    }
}
