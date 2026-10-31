use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::CommandBuffers;
use crate::{Error, Result};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn graphics_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            79 => 16,
            80 | 83 => 4,
            84 | 101 => 20,
            100 => 12,
            _ => unreachable!(),
        };
        if arguments.len() < 4 || arguments.len() > count || !arguments.len().is_multiple_of(4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = u32::from_le_bytes(arguments[..4].try_into().unwrap());
        if target == 0 {
            return Ok(NativeReturn::Continue);
        }
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let word = |i: usize| u32::from_le_bytes(arguments[i * 4..i * 4 + 4].try_into().unwrap());
        match index {
            79 => {
                let mut rectangle = [0; 32];
                rectangle[12..20].copy_from_slice(&arguments[4..12]);
                rectangle[28..32].copy_from_slice(&arguments[12..16]);
                self.command_target_with_buffers(
                    target,
                    42,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut rectangle),
                    },
                )?;
            }
            80 | 83 => {
                self.command_target(target, if index == 80 { 45 } else { 68 }, [0; 3])?;
            }
            84 | 101 => {
                let mut vector = [0; 12];
                vector.copy_from_slice(&arguments[4..16]);
                self.command_target_with_buffers(
                    target,
                    if index == 84 { 51 } else { 66 },
                    [word(4), 0, 0],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut vector),
                    },
                )?;
            }
            100 => {
                self.command_target(target, 59, [word(2), word(1), 0])?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
