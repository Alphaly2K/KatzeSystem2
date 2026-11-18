use super::{BaseObject, ObjectFactory, ObjectHost, TaskObject, kind};
use crate::kcs::{Host, Interpreter, NativeReturn, decode_utf8_units, format_string};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    fn query_quiz_debug(&mut self, index: u32, target: u32, pc: usize) -> Result<NativeReturn> {
        let receiver = self.tree.find_reachable(self.host.task_id, target);
        if !receiver.is_some_and(|node| node.kind == kind::QUIZ_DEBUG)
            || !self
                .objects
                .get(&target)
                .is_some_and(|object| object.control().deletion_requested == 0)
        {
            // 无结果时原包装会将调用者指针压栈。
            return Err(Error::new(pc, "问答调试查询没有可用的结果接收者"));
        }
        let command = [520, 522, 521, 523][(index - 839) as usize];
        Ok(NativeReturn::Value(
            self.command_target(target, command, [0; 3])?,
        ))
    }

    pub(super) fn debug_window_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 837 && matches!(arguments.len(), 4 | 8) && read::u32_at(arguments, 0)? == 0 {
            return Ok(NativeReturn::Continue);
        }
        let count = if matches!(index, 280..=282 | 288 | 837 | 838) {
            8
        } else {
            4
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if matches!(index, 839..=842) {
            return self.query_quiz_debug(index, target, vm.pc);
        } else if index == 281 {
            let address = read::u32_at(arguments, 4)?;
            if vm.read(address, 1).is_err() {
                return Ok(NativeReturn::Continue);
            }
            let mut record = vm.read(address, 352)?.to_vec();
            self.command_target_with_buffers(
                target,
                330,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
            )?;
            self.command_target(target, 333, [0, vm.stack_bytes().len() as u32, 0])?;
        } else if index == 282 {
            let source = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
            if source.len() >= 128 {
                return Err(Error::new(vm.pc, "调试窗口文字超过基线缓冲区范围"));
            }
            let mut text: Vec<u8> = decode_utf8_units(&source, false)?
                .into_iter()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            self.command_target_with_buffers(
                target,
                331,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut text),
                    ..Default::default()
                },
            )?;
        } else {
            let code = match index {
                279 => 328,
                280 => 329,
                283 => 332,
                287 => 334,
                288 => 335,
                837 => 518,
                838 => 519,
                843 => 524,
                _ => unreachable!(),
            };
            let value = if count == 8 {
                read::u32_at(arguments, 4)?
            } else {
                0
            };
            self.command_target(target, code, [0, value, 0])?;
        }
        Ok(NativeReturn::Continue)
    }
}

#[derive(Default)]
pub(super) struct QuizDebugObject(BaseObject);

impl TaskObject for QuizDebugObject {
    fn control(&self) -> &Control {
        self.0.control()
    }

    fn clock(&self) -> &TaskClock {
        self.0.clock()
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.0.state()
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.0.set_creation_context(context)
    }

    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.0.update_body(host, increment, budget)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.0.control.deletion_requested == 0 {
            match command.code {
                518 => return Err(Error::new(0, "问答调试界面尚未接入")),
                519 => return Ok(CommandOutcome::finished(false)),
                520..=524 => {
                    *output = command.data[1];
                    return Ok(CommandOutcome::finished(false));
                }
                _ => {}
            }
        }
        self.0.dispatch_command(kind, command, output, host, budget)
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.0.enqueue_message(message)
    }

    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.0.dispatch_message(message, output, host, budget)
    }

    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.0.request_delete(host, budget)
    }
}
