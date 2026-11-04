use super::{HostState, Interpreter, NativeReturn};
use crate::{Error, Result, read};

#[derive(Clone)]
pub(super) struct RecordStack {
    stride: u32,
    count: u32,
    growth: u32,
    capacity: u32,
    bytes: Vec<u8>,
}

impl RecordStack {
    fn new(stride: u32, growth: u32) -> Result<Self> {
        let length = allocation_size(stride, growth)?;
        Ok(Self {
            stride,
            count: 0,
            growth,
            capacity: growth,
            bytes: vec![0; length],
        })
    }

    fn prepare_push(&mut self) -> Result<std::ops::Range<usize>> {
        // 容量比较使用有符号 DWORD，扩容发生在源地址解析之前。
        if self.count as i32 >= self.capacity as i32 {
            let capacity = self
                .capacity
                .checked_add(self.growth)
                .ok_or_else(|| Error::new(0, "记录栈容量溢出"))?;
            let length = allocation_size(self.stride, capacity)?;
            self.bytes.resize(length, 0);
            self.capacity = capacity;
        }
        let start = self.count as usize * self.stride as usize;
        let end = start + self.stride as usize;
        Ok(start..end)
    }
}

fn allocation_size(stride: u32, capacity: u32) -> Result<usize> {
    stride
        .checked_mul(capacity)
        .map(|length| length as usize)
        .filter(|&length| length <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(0, "记录栈存储超过上限"))
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let handle = read::u32_at(arguments, 0)?;
    let count = if matches!(index, 864 | 867) { 1 } else { 2 };
    if arguments.len() != count * 4 && !(index != 863 && handle == 0 && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if index == 863 {
        if host.record_stacks.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "记录栈数量超过上限"));
        }
        let stack = RecordStack::new(handle, read::u32_at(arguments, 4)?)?;
        let handle = host.handle_allocator().allocate_at(vm.pc)?;
        host.record_stacks.insert(handle, stack);
        return Ok(NativeReturn::Value(handle));
    }
    if handle == 0 {
        return Ok(if matches!(index, 864 | 868) {
            NativeReturn::Continue
        } else {
            NativeReturn::Value(0)
        });
    }
    if index == 864 {
        if host.record_stacks.remove(&handle).is_none() {
            return Err(Error::new(vm.pc, "记录栈句柄无效"));
        }
        return Ok(NativeReturn::Continue);
    }
    if index == 868 {
        let source = read::u32_at(arguments, 4)?;
        if source == 0 {
            return Ok(NativeReturn::Continue);
        }
        let source = host
            .record_stacks
            .get(&source)
            .ok_or_else(|| Error::new(vm.pc, "记录栈源句柄无效"))?
            .clone();
        let target = host
            .record_stacks
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "记录栈目标句柄无效"))?;
        *target = source;
        return Ok(NativeReturn::Continue);
    }
    let stack = host
        .record_stacks
        .get_mut(&handle)
        .ok_or_else(|| Error::new(vm.pc, "记录栈句柄无效"))?;
    if index == 867 {
        return Ok(NativeReturn::Value(stack.count));
    }
    let address = read::u32_at(arguments, 4)?;
    if index == 865 {
        let range = stack.prepare_push()?;
        if vm.read(address, 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        stack
            .bytes
            .get_mut(range)
            .ok_or_else(|| Error::new(vm.pc, "记录栈容量不足"))?
            .copy_from_slice(vm.read(address, stack.stride as usize)?);
        stack.count = stack.count.wrapping_add(1);
    } else {
        if stack.count as i32 <= 0 || vm.read(address, 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        let count = stack.count - 1;
        let start = count as usize * stack.stride as usize;
        vm.write(address, &stack.bytes[start..start + stack.stride as usize])?;
        stack.count = count;
    }
    Ok(NativeReturn::Value(1))
}
