use super::{ObjectFactory, ObjectHost, drain};
use crate::kcs::{Interpreter, MotionQueue, NativeReturn};
use crate::{Error, Result, read};

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

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn motion_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 294 {
            let handle = self.host.handle_allocator().allocate()?;
            self.resources
                .motion_controllers
                .insert(handle, MotionQueue::default());
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        let query = matches!(index, 307 | 309..=311);
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
            295 | 296 | 303 | 311 | 314 => 1,
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
            307 => {
                std::mem::swap(
                    &mut self.host.random,
                    &mut self.resources.motion_host.random,
                );
                let result = drain(self.budget, self.limit, || {
                    queue.update(
                        args[1],
                        args[2] != 0,
                        &mut self.resources.motion_host,
                        self.budget,
                    )
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
