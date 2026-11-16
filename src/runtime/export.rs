use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn export_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            690 => 8,
            691 => 4,
            692 => 12,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if index == 691 {
            let mut scratch = [0; 4096];
            let output = self.command_target_with_buffers(
                target,
                517,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut scratch),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Value(output));
        }
        let mut names = Vec::with_capacity(2);
        for offset in (4..count).step_by(4) {
            let source = format_string(vm, read::u32_at(arguments, offset)?, 0)?;
            let source = &source[..source.iter().position(|&b| b == 0).unwrap_or(source.len())];
            let mut name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置导出资源服务"))?
                .resolve(b"export", source)?;
            name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "导出资源名称超过基线缓冲区范围"));
            }
            name.push(0);
            names.push(name);
        }
        let (first, remaining) = names.split_first_mut().unwrap();
        self.command_target_with_buffers(
            target,
            516,
            [0; 3],
            &mut CommandBuffers {
                data0: remaining.first_mut().map(Vec::as_mut_slice),
                data1: Some(first),
                ..Default::default()
            },
        )?;
        Ok(NativeReturn::Continue)
    }
}
