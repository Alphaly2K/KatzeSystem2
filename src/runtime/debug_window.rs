use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, decode_utf8_units, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn debug_window_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = if matches!(index, 280..=282 | 288) {
            8
        } else {
            4
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if index == 281 {
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
