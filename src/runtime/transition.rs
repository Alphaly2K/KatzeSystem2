use super::{ObjectFactory, ObjectHost, kind};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn transition_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = if matches!(index, 196 | 199) { 8 } else { 4 };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        match index {
            196 => {
                let object_kind = self.command_target(target, 13, [0; 3])?;
                if !(17..=31).contains(&object_kind) {
                    return Ok(NativeReturn::Continue);
                }
                let (code, length) = match object_kind {
                    kind::WIPE => (280, 56),
                    kind::FADE => (281, 44),
                    _ => return Err(Error::new(vm.pc, "尚未接入该类型的转场准备参数")),
                };
                let address = read::u32_at(arguments, 4)?;
                let mut record = vm.read(address, length)?.to_vec();
                self.command_target_with_buffers(
                    target,
                    code,
                    [0, address, 0],
                    &mut CommandBuffers {
                        data1: Some(&mut record),
                        ..Default::default()
                    },
                )?;
            }
            197 | 198 => {
                self.command_target(target, index + 66, [0; 3])?;
            }
            199 => {
                let address = read::u32_at(arguments, 4)?;
                let mut record = vm.read(address, 64)?.to_vec();
                self.command_target_with_buffers(
                    target,
                    268,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut record),
                        ..Default::default()
                    },
                )?;
            }
            200 => {
                return Ok(NativeReturn::Value(
                    self.command_target(target, 265, [0; 3])?,
                ));
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
