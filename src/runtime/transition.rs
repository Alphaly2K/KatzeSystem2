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
        if matches!(index, 853..=855) {
            let target = read::u32_at(arguments, 0)?;
            let length = match index {
                853 => 24,
                854 => 4,
                _ => 8,
            };
            if arguments.len() != length && !(target == 0 && arguments.len() == 4) {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            if target != 0 {
                if index == 853 {
                    let mut record = [0; 20];
                    for (bytes, offset) in record.chunks_exact_mut(4).zip([8, 12, 4, 16, 20]) {
                        bytes.copy_from_slice(&arguments[offset..offset + 4]);
                    }
                    self.command_target_with_buffers(
                        target,
                        525,
                        [0; 3],
                        &mut CommandBuffers {
                            data1: Some(&mut record),
                            ..Default::default()
                        },
                    )?;
                } else {
                    let value = if index == 855 {
                        read::u32_at(arguments, 4)?
                    } else {
                        0
                    };
                    self.command_target(
                        target,
                        if index == 854 { 526 } else { 527 },
                        [0, value, 0],
                    )?;
                }
            }
            return Ok(NativeReturn::Continue);
        }
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
                    kind::WIPE_ALPHA => (280, 48),
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
            197 | 198 | 203 => {
                self.command_target(target, if index == 203 { 267 } else { index + 66 }, [0; 3])?;
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
