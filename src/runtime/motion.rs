use super::{ObjectFactory, ObjectHost, drain};
use crate::kcs::{Interpreter, MotionQueue, MotionState, MotionTask, NativeReturn, format_string};
use crate::{Error, Result, read, text};

const GROUPS: [(usize, usize); 9] = [
    (0, 3),
    (3, 3),
    (6, 3),
    (9, 16),
    (25, 3),
    (28, 1),
    (29, 3),
    (32, 3),
    (35, 1),
];

/// 独立运动快照；更新时重新捕获原控制器，释放不影响控制器。
pub struct MotionSnapshot {
    controller: u32,
    storage: Vec<u8>,
    size: usize,
}

impl MotionSnapshot {
    pub fn controller(&self) -> u32 {
        self.controller
    }

    /// 当前快照的有效字节，不包括上次较大快照遗留的缓冲区尾部。
    pub fn bytes(&self) -> &[u8] {
        &self.storage[..self.size]
    }

    /// 已分配字节数；重新捕获只增长容量。
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    fn motion_restore_native(
        &mut self,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let container = read::u32_at(arguments, 0)?;
        if container == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if arguments.len() != 16 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let Some(bytes) = self
            .host
            .data_container(container)?
            .record(read::u32_at(arguments, 4)?)?
        else {
            return Ok(NativeReturn::Value(0));
        };
        let bytes = bytes.to_vec();
        if self.resources.motion_snapshots.len() == read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "运动快照数量超过上限"));
        }
        let controller = read::u32_at(arguments, 8)?;
        if controller != 0 {
            self.restore_motion_records(controller, &bytes, read::u32_at(arguments, 12)?)?;
        }
        let size = bytes.len();
        let snapshot = MotionSnapshot {
            controller,
            storage: bytes,
            size,
        };
        let handle = self.host.handle_allocator().allocate()?;
        self.resources.motion_snapshots.insert(handle, snapshot);
        Ok(NativeReturn::Value(handle))
    }

    fn restore_motion_records(&mut self, controller: u32, bytes: &[u8], now: u32) -> Result<()> {
        let count = read::u32_at(bytes, 4)? as usize;
        if count > read::MAX_ITEMS {
            return Err(Error::new(4, "运动快照记录数量超过上限"));
        }
        self.resources
            .motion_controllers
            .get_mut(&controller)
            .ok_or_else(|| Error::new(0, "运动控制器句柄无效"))?
            .clear();
        let mut offset = 8;
        for _ in 0..count {
            let record = read::range(bytes, offset, 0x454)?;
            let size = read::sized(read::u32_at(record, 0)?, offset)?;
            if size == 0 {
                offset += 0x454;
                continue;
            }
            let name: Vec<_> = record[4..0x44]
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
                .take_while(|&unit| unit != 0)
                .collect();
            if name.len() == 32 {
                return Err(Error::new(offset + 4, "运动快照名称缺少结束单元"));
            }
            let text = String::from_utf16(&name)
                .map_err(|_| Error::new(offset + 4, "运动快照名称不是有效 UTF-16"))?;
            let (encoded, _, errors) = encoding_rs::SHIFT_JIS.encode(&text);
            if errors {
                return Err(Error::new(offset + 4, "运动快照名称无法转换为 CP932"));
            }
            if !name.is_empty() {
                let resolved = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置运动资源服务"))?
                    .resolve(b"mot", &encoded)?;
                if let Some(program) = self.resources.load_motion(&resolved)? {
                    let mut state = MotionState::default();
                    for (parameter, bytes) in state
                        .parameters
                        .iter_mut()
                        .zip(record[0x48..0x448].chunks_exact(4))
                    {
                        *parameter = u32::from_le_bytes(bytes.try_into().unwrap());
                    }
                    let mut task = MotionTask::new(
                        program,
                        state,
                        now.wrapping_sub(read::u32_at(record, 0x44c)?),
                    )?;
                    task.snapshot_name = name;
                    task.snapshot_flags = read::u32_at(record, 0x450)?;
                    let length = read::sized(read::u32_at(record, 0x448)?, offset + 0x448)?;
                    if length != 0 {
                        let data = read::range(bytes, offset + 0x454, length)?.to_vec();
                        let address = task.interpreter.allocate_bytes(data)?;
                        task.interpreter
                            .write(0x8000_049c, &address.to_le_bytes())?;
                        task.interpreter
                            .write(0x8000_04a0, &(length as u32).to_le_bytes())?;
                        task.snapshot_data = Some((address, length));
                    }
                    self.resources
                        .motion_controllers
                        .get_mut(&controller)
                        .unwrap()
                        .insert(task, read::u32_at(record, 0x44)? as i32)?;
                }
            }
            offset = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(offset, "运动快照记录偏移溢出"))?;
        }
        Ok(())
    }

    fn motion_start_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        if handle == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if !self.resources.motion_controllers.contains_key(&handle) {
            return Err(Error::new(vm.pc, "运动控制器句柄无效"));
        }
        let count = if index == 535 { 8 } else { 7 };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 8];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        let name = format_string(vm, args[1], 0)?;
        let name = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置运动资源服务"))?
            .resolve(b"mot", &name)?;
        let name = &name[..name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len())];
        if name.is_empty() {
            return Ok(NativeReturn::Value(0));
        }
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "运动资源名称超过缓冲区上限"));
        }
        let Some(program) = self.resources.load_motion(name)? else {
            return Ok(NativeReturn::Value(0));
        };
        let mut state = MotionState::default();
        if args[3] != 0 {
            let parameters = vm.read(args[2], args[3].min(256) as usize * 4)?;
            for (slot, bytes) in state.parameters.iter_mut().zip(parameters.chunks_exact(4)) {
                *slot = u32::from_le_bytes(bytes.try_into().unwrap());
            }
        }
        let mut task = MotionTask::new(program, state, 0)?;
        let name: Vec<_> = text::cp932(name)?.encode_utf16().collect();
        let basename = name
            .iter()
            .rposition(|&unit| matches!(unit, 47 | 92))
            .filter(|&at| at != 0)
            .map_or(0, |at| at + 1);
        if name.len() - basename >= 32 {
            return Err(Error::new(vm.pc, "运动快照名称超过缓冲区上限"));
        }
        task.snapshot_name = name[basename..].to_vec();
        task.snapshot_flags = args[7];
        if args[5] != 0 && vm.read(args[4], 1).is_ok() {
            let data = vm.read(args[4], read::sized(args[5], vm.pc)?)?.to_vec();
            let size = data.len();
            let address = task.interpreter.allocate_bytes(data)?;
            task.interpreter
                .write(0x8000_049c, &address.to_le_bytes())?;
            task.interpreter
                .write(0x8000_04a0, &(size as u32).to_le_bytes())?;
            task.snapshot_data = Some((address, size));
        }
        let id = self
            .resources
            .motion_controllers
            .get_mut(&handle)
            .unwrap()
            .insert(task, args[6] as i32)?;
        Ok(NativeReturn::Value(id))
    }

    fn motion_snapshot_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        if index == 317 {
            if handle != 0 && self.resources.motion_snapshots.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "运动快照句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if handle == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let controller = if index == 315 {
            handle
        } else {
            self.resources
                .motion_snapshots
                .get(&handle)
                .ok_or_else(|| Error::new(vm.pc, "运动快照句柄无效"))?
                .controller
        };
        let queue = self
            .resources
            .motion_controllers
            .get(&controller)
            .ok_or_else(|| Error::new(vm.pc, "运动控制器句柄无效"))?;
        let now = if queue.is_empty() {
            0
        } else {
            read::u32_at(arguments, 4)?
        };
        let bytes = queue.snapshot(now)?;
        let size = bytes.len();
        if index == 315 {
            if self.resources.motion_snapshots.len() == read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "运动快照数量超过上限"));
            }
            let snapshot = MotionSnapshot {
                controller,
                storage: bytes,
                size,
            };
            let handle = self.host.handle_allocator().allocate()?;
            self.resources.motion_snapshots.insert(handle, snapshot);
            Ok(NativeReturn::Value(handle))
        } else {
            let snapshot = self.resources.motion_snapshots.get_mut(&handle).unwrap();
            if size > snapshot.storage.len() {
                snapshot.storage.resize(size, 0);
            }
            snapshot.storage[..size].copy_from_slice(&bytes);
            snapshot.size = size;
            Ok(NativeReturn::Value(1))
        }
    }

    pub(super) fn motion_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 388 {
            return self.motion_restore_native(arguments, vm);
        }
        if matches!(index, 304 | 535) {
            return self.motion_start_native(index, arguments, vm);
        }
        if matches!(index, 315..=317) {
            return self.motion_snapshot_native(index, arguments, vm);
        }
        if index == 294 {
            let handle = self.host.handle_allocator().allocate()?;
            self.resources
                .motion_controllers
                .insert(handle, MotionQueue::default());
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        let query = matches!(index, 307 | 309..=311 | 440);
        if handle == 0 {
            return Ok(if query {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        let queue = self
            .resources
            .motion_controllers
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "运动控制器句柄无效"))?;
        let count = match index {
            295 | 296 | 303 | 311 | 314 | 440 => 1,
            297 | 298 | 308..=310 | 312 | 313 | 586 => 2,
            305 | 307 => 3,
            306 => 4,
            299..=302 => 10,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 10];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        match index {
            295 => {
                self.resources.motion_controllers.remove(&handle);
                self.resources.motion_programs.clear();
            }
            296 => queue.clear(),
            297 => {
                queue.remove(args[1])?;
            }
            298 => {
                queue.remove_group(args[1] as i32)?;
            }
            299 | 301 => {
                let mut properties = [0; 36];
                for (i, (start, length)) in GROUPS.into_iter().enumerate() {
                    if i == 5 || i == 8 {
                        properties[start] = args[i + 1];
                    } else {
                        let bytes = vm.read(args[i + 1], length * 4)?;
                        for (target, bytes) in properties[start..start + length]
                            .iter_mut()
                            .zip(bytes.chunks_exact(4))
                        {
                            *target = u32::from_le_bytes(bytes.try_into().unwrap());
                        }
                    }
                }
                if index == 299 {
                    queue.base_properties = properties;
                } else {
                    queue.set_properties(properties)?;
                }
            }
            300 | 302 => {
                let properties = if index == 300 {
                    queue.base_properties
                } else {
                    *queue.properties()
                };
                for (i, (start, length)) in GROUPS.into_iter().enumerate() {
                    // 当前属性的最后一项允许地址解析失败，其余输出必须存在。
                    if index == 302 && i == 8 && vm.read(args[i + 1], 1).is_err() {
                        continue;
                    }
                    let bytes: Vec<_> = properties[start..start + length]
                        .iter()
                        .flat_map(|value| value.to_le_bytes())
                        .collect();
                    vm.write(args[i + 1], &bytes)?;
                }
            }
            303 => queue.base_properties = *queue.properties(),
            305 => {
                queue.set_time_origin((args[1] != 0).then_some(args[1]), args[2])?;
            }
            306 => {
                if queue.task(args[1]).is_some() {
                    let bytes = vm.read(args[2], args[3].min(256) as usize * 4)?;
                    let parameters: Vec<_> = bytes
                        .chunks_exact(4)
                        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                        .collect();
                    queue.set_parameters(args[1], &parameters)?;
                }
            }
            307 | 440 => {
                std::mem::swap(
                    &mut self.host.random,
                    &mut self.resources.motion_host.random,
                );
                let result = drain(self.budget, self.limit, || {
                    if index == 440 {
                        queue.finish(&mut self.resources.motion_host, self.budget)
                    } else {
                        queue.update(
                            args[1],
                            args[2] != 0,
                            &mut self.resources.motion_host,
                            self.budget,
                        )
                    }
                });
                std::mem::swap(
                    &mut self.host.random,
                    &mut self.resources.motion_host.random,
                );
                self.host
                    .debug_messages
                    .append(&mut self.resources.motion_host.debug_messages);
                result?;
                return Ok(NativeReturn::Value(queue.was_updated() as u32));
            }
            308 => queue.speed = f32::from_bits(args[1]),
            586 => queue.external_value = args[1],
            309 => {
                return Ok(NativeReturn::Value(match queue.task(args[1]) {
                    Some(task) => task.state()?.running as u32,
                    None => 0,
                }));
            }
            310 | 311 => {
                return Ok(NativeReturn::Value(
                    queue.any_running((index == 310).then_some(args[1] as i32))? as u32,
                ));
            }
            312 => {
                queue.request_finish(args[1])?;
            }
            313 | 314 => {
                queue.request_finish_group((index == 313).then_some(args[1] as i32))?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
