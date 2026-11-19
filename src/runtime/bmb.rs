use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn bmb_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let target = read::u32_at(arguments, 0)?;
        let count = if matches!(index, 983 | 985) { 8 } else { 4 };
        if arguments.len() != count && !(target == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if target == 0 {
            return Ok(NativeReturn::Continue);
        }

        let code = 567 + index - 983;
        if matches!(index, 983 | 984) {
            let shared = self.resources.bmb_buffer.clone();
            let mut buffer = shared
                .try_borrow_mut()
                .map_err(|_| Error::new(vm.pc, "BMB 缓冲区正在分派"))?;
            const BUFFER_SIZE: usize = 1024;
            buffer.resize(BUFFER_SIZE, 0);
            if index == 983 {
                let name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                if name.len() >= 260 {
                    return Err(Error::new(vm.pc, "BMB 源名称超过缓冲区容量"));
                }
                let name = name.split(|&b| b == 0).next().unwrap();
                // The original resolver clears the destination on failure and still dispatches.
                let resolved = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置 BMB 资源服务"))?
                    .resolve(b"bmb", name)
                    .unwrap_or_default();
                let resolved = resolved.split(|&b| b == 0).next().unwrap();
                if resolved.len() >= BUFFER_SIZE {
                    return Err(Error::new(vm.pc, "BMB 资源名称超过缓冲区容量"));
                }
                buffer[..resolved.len()].copy_from_slice(resolved);
                buffer[resolved.len()] = 0;
            }
            self.command_target_with_buffers(
                target,
                code,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut buffer),
                    ..CommandBuffers::default()
                },
            )?;
        } else {
            let value = if index == 985 {
                read::u32_at(arguments, 4)?
            } else {
                0
            };
            self.command_target(target, code, [0, value, 0])?;
        }
        Ok(NativeReturn::Continue)
    }
}
