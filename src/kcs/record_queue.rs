use super::{HostState, Interpreter, NativeReturn, native};
use crate::{Error, Result, read};
use std::collections::VecDeque;

pub(super) struct RecordQueue {
    width: usize,
    records: VecDeque<Vec<u8>>,
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    use NativeReturn::{Continue, Value};
    let offset = vm.pc;
    let first = read::u32_at(arguments, 0)?;
    let count = if matches!(index, 1012 | 1013) { 2 } else { 1 };
    if first == 0 && index != 1010 && arguments.len() == 4 {
        return Ok(if index == 1013 { Value(0) } else { Continue });
    }
    let args = native::words(arguments, count, offset)?;
    if index == 1010 {
        if host.record_queues.len() >= read::MAX_ITEMS {
            return Err(Error::new(offset, "记录队列数量超过上限"));
        }
        let width = read::sized(first, offset)?;
        let handle = host.handle_allocator().allocate_at(offset)?;
        host.record_queues.insert(
            handle,
            RecordQueue {
                width,
                records: VecDeque::new(),
            },
        );
        return Ok(Value(handle));
    }
    let empty = if index == 1013 { Value(0) } else { Continue };
    if first == 0 {
        return Ok(empty);
    }
    let invalid = || Error::new(offset, "记录队列句柄无效");
    if index == 1011 {
        host.record_queues.remove(&first).ok_or_else(invalid)?;
        return Ok(Continue);
    }
    let queue = host.record_queues.get_mut(&first).ok_or_else(invalid)?;
    let address = args[1];
    if vm.read(address, 1).is_err() {
        return Ok(empty);
    }
    if index == 1012 {
        if queue.records.len() >= read::MAX_ITEMS
            || (queue.records.len() + 1).saturating_mul(queue.width) > read::MAX_BYTES
        {
            return Err(Error::new(offset, "记录队列存储超过上限"));
        }
        queue
            .records
            .push_back(vm.read(address, queue.width)?.to_vec());
        return Ok(Continue);
    }
    let Some(record) = queue.records.front() else {
        return Ok(Value(0));
    };
    vm.write(address, record)?;
    queue.records.pop_front();
    Ok(Value(1))
}
