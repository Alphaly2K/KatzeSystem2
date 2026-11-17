use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn screen_filter_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let target = read::u32_at(arguments, 0)?;
        if target == 0 {
            return Ok(NativeReturn::Continue);
        }
        let length = match index {
            521 => 32,
            523 => 40,
            525 => 16,
            _ => 8,
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if matches!(index, 521 | 523 | 525) {
            let mut record = if index == 521 {
                vm.read(read::u32_at(arguments, 4)?, 64)?.to_vec()
            } else {
                Vec::new()
            };
            record.extend_from_slice(&arguments[if index == 521 { 8 } else { 4 }..]);
            self.command_target_with_buffers(
                target,
                index - 137,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
            )?;
        } else {
            self.command_target(target, index - 137, [0, read::u32_at(arguments, 4)?, 0])?;
        }
        Ok(NativeReturn::Continue)
    }
}
