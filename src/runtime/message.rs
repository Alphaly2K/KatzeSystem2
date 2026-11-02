use super::{ObjectFactory, ObjectHost, route_command};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn message_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let length = match index {
            611..=614 | 619 | 695 => 8,
            615..=618 | 884 => 4,
            _ => unreachable!(),
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        match index {
            611..=613 => {
                let address = read::u32_at(arguments, 4)?;
                let length = if read::u32_at(vm.read(address, 4)?, 0)? == 0xe8 {
                    0xe8
                } else {
                    4
                };
                let mut buffer = vm.read(address, length)?.to_vec();
                let result = self.command_target_with_buffers(
                    target,
                    455 + index - 611,
                    [0, address, 0],
                    &mut CommandBuffers {
                        data1: Some(&mut buffer),
                        ..CommandBuffers::default()
                    },
                );
                vm.write(address, &buffer)?;
                result?;
            }
            614 => {
                let mut text = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                text.truncate(
                    text.iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(text.len()),
                );
                text.push(0);
                self.command_target_with_buffers(
                    target,
                    458,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut text),
                        ..CommandBuffers::default()
                    },
                )?;
            }
            616 | 884 => {
                let sender = self.host.task_id;
                let supported = self
                    .tree
                    .find_reachable(sender, target)
                    .is_some_and(|node| node.kind == super::kind::STRING_PLANE);
                let deleted = self
                    .objects
                    .get(&target)
                    .is_some_and(|object| object.control().deletion_requested != 0);
                if !supported || deleted {
                    return Err(Error::new(
                        vm.pc,
                        "消息数量查询目标不可达、已删除或类型未接入",
                    ));
                }
                let mut output = 0;
                let found = route_command(
                    self.store(),
                    sender,
                    Command {
                        target,
                        sender,
                        code: if index == 616 { 460 } else { 472 },
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut output,
                )?;
                if !found {
                    return Err(Error::new(vm.pc, "消息数量查询的原返回槽未写入"));
                }
                return Ok(NativeReturn::Value(output));
            }
            615 | 617 | 618 | 619 | 695 => {
                let code = match index {
                    615 => 459,
                    617 => 461,
                    618 => 462,
                    619 => 463,
                    695 => 467,
                    _ => unreachable!(),
                };
                let value = if arguments.len() == 8 {
                    read::u32_at(arguments, 4)?
                } else {
                    0
                };
                self.command_target(target, code, [0, value, 0])?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
