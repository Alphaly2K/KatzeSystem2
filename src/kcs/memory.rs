use super::Interpreter;
use crate::Result;

/// 基线复制入口根据系统上下文及 MMX 能力选择的分支。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryCopyMode {
    Crt,
    Dword,
    #[default]
    Mmx,
}

pub(crate) fn copy(
    vm: &mut Interpreter,
    destination: u32,
    source: u32,
    count: usize,
    mode: MemoryCopyMode,
) -> Result<()> {
    vm.read(source, count)?;
    vm.read(destination, count)?;
    let block = if mode == MemoryCopyMode::Mmx && count & 7 == 0 {
        8
    } else if mode != MemoryCopyMode::Crt && count & 3 == 0 {
        4
    } else {
        let bytes = vm.read(source, count)?.to_vec();
        return vm.write(destination, &bytes);
    };
    let mut bytes = [0; 8];
    for offset in (0..count).step_by(block) {
        bytes[..block].copy_from_slice(vm.read(source + offset as u32, block)?);
        vm.write(destination + offset as u32, &bytes[..block])?;
    }
    Ok(())
}
