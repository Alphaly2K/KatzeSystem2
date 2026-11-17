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
            144 => (127, 16),
            145 => (128, 0),
            146 => (129, 8),
            147 => (130, 8),
            148 => (132, 8),
            150 => (138, 12),
            154 => (143, 8),
            285 => (145, 4),
            286 => (146, 0),
            396 => (149, 0),
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
            144 => [b, 0, 0],
            145 => [0, self.host.task_id, 0],
            401 => [0, a, 0],
            147 => [a, u32::from(b != 0), 0],
            150 => [read::u32_at(arguments, 8)?, (a & 0xffff) | (b << 16), 0],
            _ => [a, b, 0],
        };
        let sender = self.host.task_id;
        let mut buffer = if index == 144 {
            let mut record = vec![0; 60];
            record[..4].copy_from_slice(&sender.to_le_bytes());
            record[8..12].copy_from_slice(&arguments[12..16]);
            let address = read::u32_at(arguments, 8)?;
            if address != 0 {
                record[12..28].copy_from_slice(&vm.read(address, 16)?);
            }
            Some(record)
        } else {
            None
        };
        let target = self.command_target(0, 34, [0; 3])?;
        if index == 147 && b != 0 {
            buffer = Some(vm.read(b, 16)?.to_vec());
        }
        let mut cursor = [0; 4];
        if index == 396
            && !self
                .tree
                .node(target)
                .is_some_and(|node| node.kind == crate::runtime::kind::INPUT)
        {
            return Err(Error::new(vm.pc, "KCS 光标状态查询缺少输入任务"));
        }
        let mut output = 0;
        let found = route_command(
            self.store(),
            sender,
            Command {
                target,
                sender: if index == 144 { a } else { sender },
                code,
                data,
            },
            &mut CommandBuffers {
                data1: if index == 396 {
                    Some(&mut cursor)
                } else {
                    buffer.as_deref_mut()
                },
                ..CommandBuffers::default()
            },
            &mut output,
        )?;
        Ok(match index {
            285 => NativeReturn::Value(output),
            396 if found => NativeReturn::Value(u32::from_le_bytes(cursor)),
            396 => return Err(Error::new(vm.pc, "KCS 光标状态查询缺少输入任务")),
            _ => NativeReturn::Continue,
        })
    }
}
