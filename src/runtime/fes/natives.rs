use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(in crate::runtime) fn fes_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if matches!(index, 649 | 650) {
            return self.fes_local_record_native(index, arguments, vm);
        }
        let length = match index {
            448 => 4,
            630 => 16,
            645 | 882 => 12,
            _ => 8,
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        match index {
            447 => {
                let formatted = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                let mut name = self
                    .resources
                    .services
                    .as_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置 FES 资源服务"))?
                    .resolve(b"fes", &formatted)?;
                name.truncate(
                    name.iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(name.len()),
                );
                name.push(0);
                // 共享数值上下文由运行时持有，337 以零选择该状态。
                self.command_target(target, 337, [0; 3])?;
                self.command_target_with_buffers(
                    target,
                    338,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut name),
                        ..CommandBuffers::default()
                    },
                )?;
            }
            448 => {
                self.command_target(target, 339, [0; 3])?;
            }
            449 => {
                self.command_target(target, 340, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            451 => {
                self.command_target(target, 342, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            647 => {
                self.command_target(target, 348, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            630 => {
                let mut name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                if std::env::var_os("KATZE_TRACE_NATIVE").is_some() {
                    eprintln!(
                        "[fes-call] caller={} pc={} target={target} block={:?}",
                        self.host.task_id,
                        vm.pc,
                        String::from_utf8_lossy(&name),
                    );
                }
                name.push(0);
                let prefix = format_string(vm, read::u32_at(arguments, 8)?, 0)?;
                let mut context = [0; 36];
                if prefix.len() >= 32 {
                    return Err(Error::new(vm.pc, "FES 块对象前缀超过容量"));
                }
                context[..prefix.len()].copy_from_slice(&prefix);
                context[32..].copy_from_slice(&read::u32_at(arguments, 12)?.to_le_bytes());
                self.command_target_with_buffers(
                    target,
                    343,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: if prefix.is_empty() {
                            None
                        } else {
                            Some(&mut context)
                        },
                        data1: Some(&mut name),
                        external: None,
                    },
                )?;
            }
            645 => {
                self.command_target(
                    target,
                    346,
                    [read::u32_at(arguments, 4)?, read::u32_at(arguments, 8)?, 0],
                )?;
            }
            646 => {
                let supported = self
                    .tree
                    .find_reachable(self.host.task_id, target)
                    .is_some_and(|node| node.kind == crate::runtime::kind::FES);
                let active = self
                    .objects
                    .get(&target)
                    .map(|object| object.control().deletion_requested == 0)
                    .unwrap_or_else(|| self.running.get(&target).is_some_and(|task| !task.deleted));
                if !supported || !active {
                    return Err(Error::new(
                        vm.pc,
                        "FES 参数查询目标不可达、已删除或类型未接入",
                    ));
                }
                return Ok(NativeReturn::Value(self.command_target(
                    target,
                    347,
                    [read::u32_at(arguments, 4)?, 0, 0],
                )?));
            }
            882 | 883 => {
                let mut name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                name.truncate(
                    name.iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(name.len()),
                );
                name.push(0);
                if index == 882 {
                    self.command_target_with_buffers(
                        target,
                        358,
                        [0, read::u32_at(arguments, 8)?, 0],
                        &mut CommandBuffers {
                            data0: Some(&mut name),
                            ..CommandBuffers::default()
                        },
                    )?;
                } else {
                    let supported = self
                        .tree
                        .find_reachable(self.host.task_id, target)
                        .is_some_and(|node| node.kind == crate::runtime::kind::FES);
                    let active = self.objects.get(&target).is_some_and(|object| {
                        object.control().deletion_requested == 0
                            && object.shared_numbers_available() == Some(true)
                    });
                    if !supported || !active {
                        return Err(Error::new(vm.pc, "FES 名称查询需要可达且已初始化的目标"));
                    }
                    let mut value = [0; 4];
                    self.command_target_with_buffers(
                        target,
                        357,
                        [0; 3],
                        &mut CommandBuffers {
                            data0: Some(&mut name),
                            data1: Some(&mut value),
                            external: None,
                        },
                    )?;
                    return Ok(NativeReturn::Value(u32::from_le_bytes(value)));
                }
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
    fn fes_local_record_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let container = read::u32_at(arguments, 0)?;
        if container == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if arguments.len() != 12 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let record = read::u32_at(arguments, 4)?;
        let target = read::u32_at(arguments, 8)?;
        if index == 650 {
            let Some(bytes) = self.host.data_container(container)?.record(record)? else {
                return Ok(NativeReturn::Value(0));
            };
            let mut bytes = bytes.to_vec();
            self.command_target_with_buffers(
                target,
                350,
                [bytes.len() as u32, 0, 0],
                &mut CommandBuffers {
                    data1: Some(&mut bytes),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Value(1));
        }
        let size = self.command_target(target, 349, [0; 3])? as usize;
        if size == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if size > read::MAX_BYTES {
            return Err(Error::new(vm.pc, "FES 参数记录超过存储上限"));
        }
        let mut bytes = vec![0; size];
        let copied = self.command_target_with_buffers(
            target,
            349,
            [size as u32, 0, 0],
            &mut CommandBuffers {
                data1: Some(&mut bytes),
                ..Default::default()
            },
        )? as usize;
        if copied > bytes.len() {
            return Err(Error::new(vm.pc, "FES 参数记录返回长度超过缓冲区"));
        }
        self.host
            .data_container_mut(container)?
            .append(record, &bytes[..copied])?;
        Ok(NativeReturn::Value(1))
    }
}
