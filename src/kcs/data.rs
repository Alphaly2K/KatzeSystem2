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
        244 | 254 | 835 => 4,
        245 | 469 => 2,
        242 | 836 => 3,
        252 => 2,
        _ => 1,
    };
    let first = arguments
        .get(..4)
        .map(|a| u32::from_le_bytes(a.try_into().unwrap()))
        .ok_or_else(|| Error::new(vm.pc, "KCS native 参数字节数不匹配"))?;
    let null = first == 0 && matches!(index, 244 | 245 | 252 | 254 | 469 | 835 | 836);
    if arguments.len() != count * 4 && !(null && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if index == 833 {
        return Ok(NativeReturn::Value(
            host.insert_raw_buffer(vec![0; read::sized(first, vm.pc)?], vm.pc)?,
        ));
    }
    if matches!(index, 290 | 834) {
        if first != 0 && host.raw_buffers.remove(&first).is_none() {
            return Err(Error::new(vm.pc, "原始缓冲区释放地址无效"));
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 238 | 247 | 252) {
        let container = if index == 238 {
            DataContainer::new(first)
        } else if index == 252 {
            if first == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let Some(data) = host
                .data_container(first)?
                .record(read::u32_at(arguments, 4)?)?
            else {
                return Ok(NativeReturn::Value(0));
            };
            DataContainer::from_snapshot(data)?
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
    if index == 245 {
        let name = strings::format(vm, read::u32_at(arguments, 4)?, 0)?;
        let bytes = host.data_container(first)?.to_css()?;
        let saved = host
            .save_services
            .as_mut()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置存档写入服务"))?
            .write(host.task_id, &name, &bytes)?;
        return Ok(NativeReturn::Value(u32::from(saved)));
    }
    let args: Vec<_> = arguments
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if index == 242 {
        if args[2] == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let data = host.data_container(args[2])?.snapshot()?;
        host.data_container_mut(first)?.append(args[1], &data)?;
        return Ok(NativeReturn::Value(1));
    }
    if index == 835 {
        if args[3] == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let data = host
            .raw_buffer(args[3], read::sized(args[2], vm.pc)?)?
            .to_vec();
        host.data_container_mut(first)?.append(args[1], &data)?;
        return Ok(NativeReturn::Value(1));
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
    if index == 469 {
        container.remove(args[1])?;
        return Ok(NativeReturn::Continue);
    }
    if index == 244 {
        if vm.read(args[3], 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        container.append(args[1], &vm.read(args[3], args[2] as usize)?)?;
        return Ok(NativeReturn::Value(1));
    }
    let Some(data) = container.record(args[1])? else {
        return Ok(NativeReturn::Value(0));
    };
    if index == 836 {
        let data = data.to_vec();
        let length = data.len() as u32;
        let address = host.insert_raw_buffer(data, vm.pc)?;
        vm.write(args[2], &length.to_le_bytes())?;
        return Ok(NativeReturn::Value(address));
    }
    if vm.read(args[2], 1).is_err() {
        return Ok(NativeReturn::Value(0));
    }
    let length = data.len().min(args[3] as usize);
    vm.write(args[2], &data[..length])?;
    Ok(NativeReturn::Value(length as u32))
}

impl HostState {
    pub(super) fn insert_raw_buffer(&mut self, bytes: Vec<u8>, offset: usize) -> Result<u32> {
        if bytes.len() > read::MAX_BYTES || self.raw_buffers.len() == read::MAX_ITEMS {
            return Err(Error::new(offset, "原始缓冲区超过存储上限"));
        }
        let address = self
            .handle_allocator()
            .allocate_buffer_at(bytes.len(), offset)?;
        self.raw_buffers.insert(address, bytes);
        Ok(address)
    }

    /// 读取独立原始缓冲区的地址范围；地址允许包含块内偏移。
    pub fn raw_buffer(&self, address: u32, length: usize) -> Result<&[u8]> {
        read::range(self.raw_buffer_rest(address)?, 0, length)
    }

    pub(super) fn raw_buffer_rest(&self, address: u32) -> Result<&[u8]> {
        let (&base, bytes) = self
            .raw_buffers
            .range(..=address)
            .next_back()
            .ok_or_else(|| Error::new(address as usize, "原始缓冲区地址无效"))?;
        bytes
            .get((address - base) as usize..)
            .ok_or_else(|| Error::new(address as usize, "原始缓冲区地址越界"))
    }

    /// 写入独立原始缓冲区；不得将原始地址用作 VM 本地或外部区地址。
    pub fn raw_buffer_mut(&mut self, address: u32, length: usize) -> Result<&mut [u8]> {
        let (&base, bytes) = self
            .raw_buffers
            .range_mut(..=address)
            .next_back()
            .ok_or_else(|| Error::new(address as usize, "原始缓冲区地址无效"))?;
        let start = (address - base) as usize;
        let end = start
            .checked_add(length)
            .ok_or_else(|| Error::new(start, "原始缓冲区范围溢出"))?;
        bytes
            .get_mut(start..end)
            .ok_or_else(|| Error::new(start, "原始缓冲区写入越界"))
    }
}
