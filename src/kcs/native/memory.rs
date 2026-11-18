use crate::kcs::{HostState, Interpreter, NativeReturn, memory};
use crate::{Result, read};

pub(super) fn call(
    host: &HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let args = super::words(
        arguments,
        if matches!(index, 27 | 28 | 237) { 1 } else { 3 },
        vm.pc,
    )?;
    Ok(match index {
        27 => {
            let length = read::sized(args[0], vm.pc)?;
            NativeReturn::Value(if length == 0 {
                u32::MAX
            } else {
                vm.allocate_bytes(vec![0; length])?
            })
        }
        28 => NativeReturn::Value(u32::from(vm.release_bytes(args[0]))),
        237 => {
            vm.release_bytes(args[0]);
            NativeReturn::Continue
        }
        35 => {
            memory::copy(
                vm,
                args[0],
                args[1],
                read::sized(args[2], vm.pc)?,
                host.memory_copy_mode,
            )?;
            NativeReturn::Continue
        }
        36 => {
            let count = read::sized(args[2], vm.pc)?;
            vm.read(args[0], count)?;
            vm.write(args[0], &vec![args[1] as u8; count])?;
            NativeReturn::Continue
        }
        _ => unreachable!(),
    })
}
