use super::Interpreter;
use crate::{Error, Result, read};

pub(crate) struct MemoryLoan {
    memory: Vec<u8>,
    allocations: std::collections::BTreeMap<usize, Vec<u8>>,
    external: crate::memory::SharedMemory,
}

impl Interpreter {
    pub(crate) fn lend_memory(&mut self) -> MemoryLoan {
        MemoryLoan {
            memory: std::mem::take(&mut self.memory),
            allocations: std::mem::take(&mut self.allocations),
            external: self.external.clone(),
        }
    }

    pub(crate) fn restore_memory(&mut self, loan: MemoryLoan) {
        self.memory = loan.memory;
        self.allocations = loan.allocations;
    }
}

impl MemoryLoan {
    pub(crate) fn read(
        &self,
        address: u32,
        length: usize,
    ) -> Result<crate::memory::MemoryRead<'_>> {
        let offset = (address & 0x7fff_ffff) as usize;
        if address & 0x8000_0000 != 0 {
            return Ok(crate::memory::MemoryRead::Shared(
                self.external.read(offset, length)?,
            ));
        }
        let (bytes, offset) = if offset < self.memory.len() || self.allocations.is_empty() {
            (&self.memory, offset)
        } else {
            let (base, bytes) = self
                .allocations
                .range(..=offset)
                .next_back()
                .ok_or_else(|| Error::new(offset, "KCS 分配地址无效"))?;
            (bytes, offset - base)
        };
        Ok(crate::memory::MemoryRead::Local(read::range(
            bytes, offset, length,
        )?))
    }

    pub(crate) fn write(&mut self, address: u32, value: &[u8]) -> Result<()> {
        let offset = (address & 0x7fff_ffff) as usize;
        if address & 0x8000_0000 != 0 {
            return self.external.write(offset, value);
        }
        let (bytes, offset) = if offset < self.memory.len() || self.allocations.is_empty() {
            (&mut self.memory, offset)
        } else {
            let (base, bytes) = self
                .allocations
                .range_mut(..=offset)
                .next_back()
                .ok_or_else(|| Error::new(offset, "KCS 分配地址无效"))?;
            (bytes, offset - *base)
        };
        let end = offset
            .checked_add(value.len())
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| Error::new(offset, "KCS 写入越界"))?;
        bytes[offset..end].copy_from_slice(value);
        Ok(())
    }
}

pub(crate) fn copy_loan(
    vm: &mut Interpreter,
    loan: &mut MemoryLoan,
    other: u32,
    current: u32,
    count: usize,
    mode: MemoryCopyMode,
    to_other: bool,
) -> Result<()> {
    vm.read(current, count)?;
    loan.read(other, count)?;
    let block = if mode == MemoryCopyMode::Mmx && count & 7 == 0 {
        8
    } else if mode != MemoryCopyMode::Crt && count & 3 == 0 {
        4
    } else {
        count.max(1)
    };
    for offset in (0..count).step_by(block) {
        if to_other {
            let bytes = vm.read(current + offset as u32, block)?.to_vec();
            loan.write(other + offset as u32, &bytes)?;
        } else {
            let bytes = loan.read(other + offset as u32, block)?.to_vec();
            vm.write(current + offset as u32, &bytes)?;
        }
    }
    Ok(())
}

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
        bytes[..block].copy_from_slice(&vm.read(source + offset as u32, block)?);
        vm.write(destination + offset as u32, &bytes[..block])?;
    }
    Ok(())
}

pub(crate) fn copy_between(
    destination_vm: &mut Interpreter,
    destination: u32,
    source_vm: &Interpreter,
    source: u32,
    count: usize,
    mode: MemoryCopyMode,
) -> Result<()> {
    source_vm.read(source, count)?;
    destination_vm.read(destination, count)?;
    let block = if mode == MemoryCopyMode::Mmx && count & 7 == 0 {
        8
    } else if mode != MemoryCopyMode::Crt && count & 3 == 0 {
        4
    } else {
        let bytes = source_vm.read(source, count)?.to_vec();
        return destination_vm.write(destination, &bytes);
    };
    let mut bytes = [0; 8];
    for offset in (0..count).step_by(block) {
        bytes[..block].copy_from_slice(&source_vm.read(source + offset as u32, block)?);
        destination_vm.write(destination + offset as u32, &bytes[..block])?;
    }
    Ok(())
}
