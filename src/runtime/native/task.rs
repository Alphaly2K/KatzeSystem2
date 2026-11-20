use super::super::{
    Execution, ObjectFactory, ObjectHost, create_object, kind, route_command, run_nested,
};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn task_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            66 => 12,
            67 | 74 | 75 | 77 | 109 | 272..=274 | 452 | 454 | 577 | 651 => 4,
            108 | 110 | 275 | 426 | 427 | 694 => 8,
            69 | 70 | 72 | 73 => 16,
            76 => 0,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 5];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        match index {
            74 => {
                let sender = self.host.task_id;
                let mut output = 0;
                let found = route_command(
                    self.store(),
                    sender,
                    Command {
                        target: args[0],
                        sender,
                        code: 13,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut output,
                )?;
                Ok(NativeReturn::Value(if found { output } else { u32::MAX }))
            }
            77 => Ok(NativeReturn::Value(self.command_target(
                0,
                33,
                [args[0], 0, 0],
            )?)),
            577 => {
                self.command_target(0, 40, [0, args[0], 0])?;
                Ok(NativeReturn::Continue)
            }
            651 => {
                self.command_target(args[0], 24, [0, u32::MAX, 0])?;
                Ok(NativeReturn::Continue)
            }
            454 => {
                crate::kcs::write_task_context(vm, args[0], &self.context)?;
                Ok(NativeReturn::Continue)
            }
            272 | 273 => {
                self.command_target(args[0], index + 49, [0; 3])?;
                Ok(NativeReturn::Continue)
            }
            274 => Ok(NativeReturn::Value(
                self.command_target(args[0], 324, [0; 3])?,
            )),
            275 => {
                let mut parameters = vm.read(args[1], 128).ok().map(|bytes| bytes.to_vec());
                self.command_target_with_buffers(
                    args[0],
                    325,
                    [0, args[1], 0],
                    &mut CommandBuffers {
                        data1: parameters.as_deref_mut(),
                        ..CommandBuffers::default()
                    },
                )?;
                Ok(NativeReturn::Continue)
            }
            426 | 427 => {
                self.command_target(args[0], index - 100, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            452 => {
                let sender = self.host.task_id;
                if self.require_reachable(args[0])
                    && self
                        .tree
                        .node(args[0])
                        .is_some_and(|node| node.kind == kind::ORDINARY)
                {
                    if let Some(task) = self.running.get(&args[0]) {
                        return Ok(NativeReturn::Value(if task.deleted {
                            u32::MAX
                        } else {
                            task.property
                        }));
                    }
                }
                let mut output = u32::MAX;
                route_command(
                    self.store(),
                    sender,
                    Command {
                        target: args[0],
                        sender,
                        code: 23,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut output,
                )?;
                Ok(NativeReturn::Value(output))
            }
            75 | 76 => crate::kcs::hierarchy_query(
                self.tree,
                self.host.task_id,
                index,
                args.first().copied().unwrap_or(0),
                vm.pc,
            ),
            66 => {
                // 命令先路由到父任务；不可达的父级不能接受创建请求。
                if !self.require_reachable(args[1]) {
                    return Err(Error::new(vm.pc, "任务创建父级不可达"));
                }
                let name = crate::kcs::format_string(vm, args[2], 0)?;
                let end = self
                    .context
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(self.context.len());
                let context = self.context[..end].to_vec();
                let id = create_object(self.store(), args[1], args[0], &name, &context)?;
                Ok(NativeReturn::Value(id))
            }
            67 => {
                if !self.require_reachable(args[0]) {
                    return Err(Error::new(vm.pc, "任务删除目标不存在"));
                }
                if self.resources.input_callbacks.contains_key(&args[0]) {
                    run_nested(self.store(), args[0], Execution::Delete)?;
                    return Ok(NativeReturn::Continue);
                }
                if let Some(task) = self.running.get_mut(&args[0]) {
                    if task.remaining_messages.is_none() && !task.deferred_delete {
                        return Err(Error::new(vm.pc, "执行中对象不支持删除请求"));
                    }
                    if !task.deleted {
                        task.deleted = true;
                        task.effects.delete = true;
                    }
                } else {
                    run_nested(self.store(), args[0], Execution::Delete)?;
                }
                Ok(NativeReturn::Continue)
            }
            69 | 70 => {
                let mut message = [self.host.task_id, args[1], args[2], args[3]];
                if index == 70 {
                    if !self.require_reachable(args[0]) {
                        return Err(Error::new(vm.pc, "任务消息目标不存在"));
                    }
                    if let Some(task) = self.running.get_mut(&args[0]) {
                        let remaining = task
                            .remaining_messages
                            .as_mut()
                            .ok_or_else(|| Error::new(vm.pc, "执行中对象不支持排队消息"))?;
                        if task.deleted {
                            return Err(Error::new(vm.pc, "已删除任务的排队返回值未初始化"));
                        }
                        if *remaining == 0 {
                            return Err(Error::new(vm.pc, "普通任务消息数量超过上限"));
                        }
                        *remaining -= 1;
                        task.effects.messages.push(message);
                    } else {
                        let object = self
                            .objects
                            .get_mut(&args[0])
                            .ok_or_else(|| Error::new(vm.pc, "任务对象不存在"))?;
                        if object.control().deletion_requested != 0 {
                            return Err(Error::new(vm.pc, "已删除任务的排队返回值未初始化"));
                        }
                        object.enqueue_message(message)?;
                    }
                    return Ok(NativeReturn::Value(1));
                }
                // 正在执行 native 的自身 VM 不处于消息等待状态。
                let value = if args[0] == self.host.task_id {
                    0
                } else {
                    let owner = self.host.task_id;
                    self.running.get_mut(&owner).unwrap().memory = Some(vm.lend_memory());
                    let result = self.message_target(args[0], &mut message);
                    vm.restore_memory(self.running.get_mut(&owner).unwrap().memory.take().unwrap());
                    result?
                };
                Ok(NativeReturn::Value(value))
            }
            109 => Ok(NativeReturn::Value(
                self.command_target(args[0], 96, [0; 3])?,
            )),
            108 | 110 => {
                self.command_target(args[0], if index == 108 { 95 } else { 97 }, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            694 => {
                self.command_target(args[0], 29, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            72 | 73 => {
                let count = read::sized(args[2], vm.pc)?;
                if args[0] == self.host.task_id {
                    let (destination, source) = if index == 72 {
                        (args[3], args[1])
                    } else {
                        (args[1], args[3])
                    };
                    crate::kcs::copy_memory(
                        vm,
                        destination,
                        source,
                        count,
                        self.host.memory_copy_mode,
                    )?;
                } else {
                    if !self.require_reachable(args[0]) {
                        return Err(Error::new(vm.pc, "任务存储目标不存在"));
                    }
                    if let Some(loan) = self
                        .running
                        .get_mut(&args[0])
                        .and_then(|task| task.memory.as_mut())
                    {
                        crate::kcs::copy_loan(
                            vm,
                            loan,
                            args[1],
                            args[3],
                            count,
                            self.host.memory_copy_mode,
                            index == 73,
                        )?;
                        return Ok(NativeReturn::Continue);
                    }
                    let other = self
                        .objects
                        .get_mut(&args[0])
                        .and_then(|object| object.interpreter_mut())
                        .ok_or_else(|| Error::new(vm.pc, "任务对象不存在、正在执行或没有 VM"))?;
                    if index == 72 {
                        crate::kcs::copy_memory_between(
                            vm,
                            args[3],
                            other,
                            args[1],
                            count,
                            self.host.memory_copy_mode,
                        )?;
                    } else {
                        crate::kcs::copy_memory_between(
                            other,
                            args[1],
                            vm,
                            args[3],
                            count,
                            self.host.memory_copy_mode,
                        )?;
                    }
                }
                Ok(NativeReturn::Continue)
            }
            _ => unreachable!(),
        }
    }
}
