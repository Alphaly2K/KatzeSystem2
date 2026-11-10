use super::{ObjectFactory, ObjectHost, route_command};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn input_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let (code, length) = match index {
            145 => (128, 0),
            146 => (129, 8),
            147 => (130, 8),
            148 => (132, 8),
            154 => (143, 8),
            285 => (145, 4),
            286 => (146, 0),
            401 => (150, 4),
            403 => (151, 8),
            404 => (152, 0),
            937 => (158, 0),
            _ => unreachable!(),
        };
        if length != 0 && arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let a = if length >= 4 {
            read::u32_at(arguments, 0)?
        } else {
            0
        };
        let b = if length >= 8 {
            read::u32_at(arguments, 4)?
        } else {
            0
        };
        let data = match index {
            145 => [0, self.host.task_id, 0],
            401 => [0, a, 0],
            147 => [a, u32::from(b != 0), 0],
            _ => [a, b, 0],
        };
        let target = self.command_target(0, 34, [0; 3])?;
        let sender = self.host.task_id;
        let mut rectangle = if index == 147 && b != 0 {
            Some(vm.read(b, 16)?.to_vec())
        } else {
            None
        };
        let mut output = 0;
        route_command(
            self.store(),
            sender,
            Command {
                target,
                sender,
                code,
                data,
            },
            &mut CommandBuffers {
                data1: rectangle.as_deref_mut(),
                ..CommandBuffers::default()
            },
            &mut output,
        )?;
        Ok(if index == 285 {
            NativeReturn::Value(output)
        } else {
            NativeReturn::Continue
        })
    }
}
