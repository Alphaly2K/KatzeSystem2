use super::{HostState, Interpreter, NativeReturn, strings};
use crate::save::DataContainer;
use crate::{Error, Result, read};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let count = match index {
        244 | 254 => 4,
        469 => 2,
        _ => 1,
    };
    let first = arguments
        .get(..4)
        .map(|a| u32::from_le_bytes(a.try_into().unwrap()))
        .ok_or_else(|| Error::new(vm.pc, "KCS native 参数字节数不匹配"))?;
    let null = first == 0 && matches!(index, 244 | 254 | 469);
    if arguments.len() != count * 4 && !(null && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if matches!(index, 238 | 247) {
        let container = if index == 238 {
            DataContainer::new(first)
        } else {
            let name = strings::format(vm, first, 0)?;
            let bytes = host
                .save_services
                .as_mut()
                .ok_or_else(|| Error::new(vm.pc, "宿主未配置存档读取服务"))?
                .read(host.task_id, &name)?;
            let Some(bytes) = bytes else {
                return Ok(NativeReturn::Value(0));
            };
            let Some(container) = DataContainer::from_css(&bytes)? else {
                return Ok(NativeReturn::Value(0));
            };
            container
        };
        if host.containers.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "KCS 数据容器数量超过上限"));
        }
        let handle = host.handle_allocator().allocate_at(vm.pc)?;
        host.containers.insert(handle, container);
        return Ok(NativeReturn::Value(handle));
    }
    if index == 246 {
        if first != 0 && host.containers.remove(&first).is_none() {
            return Err(Error::new(vm.pc, "KCS 数据容器句柄无效"));
        }
        return Ok(NativeReturn::Continue);
    }
    if first == 0 {
        return Ok(if index == 469 {
            NativeReturn::Continue
        } else {
            NativeReturn::Value(0)
        });
    }
    let container = host
        .containers
        .get_mut(&first)
        .ok_or_else(|| Error::new(vm.pc, "KCS 数据容器句柄无效"))?;
    if index == 248 {
        return Ok(NativeReturn::Value(container.tag()));
    }
    if index == 389 {
        container.clear();
        return Ok(NativeReturn::Value(1));
    }
    let args: Vec<_> = arguments
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if index == 469 {
        container.remove(args[1])?;
        return Ok(NativeReturn::Continue);
    }
    if index == 244 {
        if vm.read(args[3], 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        container.append(args[1], vm.read(args[3], args[2] as usize)?)?;
        return Ok(NativeReturn::Value(1));
    }
    let Some(data) = container.record(args[1])? else {
        return Ok(NativeReturn::Value(0));
    };
    if vm.read(args[2], 1).is_err() {
        return Ok(NativeReturn::Value(0));
    }
    let length = data.len().min(args[3] as usize);
    vm.write(args[2], &data[..length])?;
    Ok(NativeReturn::Value(length as u32))
}
