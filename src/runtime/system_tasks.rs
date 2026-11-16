use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn system_task_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            600 => 12,
            1018 => 16,
            601 | 1022 => 4,
            1020 | 1023 => 8,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if index == 1018 {
            let mut record = [0; 2048];
            for (offset, field) in [0, 512].into_iter().enumerate() {
                let text = format_string(vm, read::u32_at(arguments, 4 + offset * 4)?, 0)?;
                let text = &text[..text
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(text.len())];
                if text.len() >= 512 {
                    return Err(Error::new(vm.pc, "网络任务文字超过字段容量"));
                }
                record[field..field + text.len()].copy_from_slice(text);
            }
            let path = format_string(vm, read::u32_at(arguments, 12)?, 0)?;
            let path = self
                .host
                .folder_paths
                .as_ref()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置目录展开上下文"))?
                .expand_bytes(&path, 511)?;
            for (bytes, unit) in record[1024..].chunks_exact_mut(2).zip(path) {
                bytes.copy_from_slice(&unit.to_le_bytes());
            }
            self.command_target_with_buffers(
                target,
                583,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Continue);
        }
        if matches!(index, 601 | 1022) {
            let mut scratch = [0; 4096];
            let result = self.command_target_with_buffers(
                target,
                if index == 601 { 454 } else { 587 },
                [0; 3],
                &mut CommandBuffers {
                    data1: (index == 601).then_some(scratch.as_mut_slice()),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Value(result));
        }
        let first = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
        let first = &first[..first
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(first.len())];
        if index != 600 {
            let mut text = first.to_vec();
            text.push(0);
            self.command_target_with_buffers(
                target,
                if index == 1020 { 585 } else { 588 },
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut text),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Continue);
        }
        let name: Vec<_> = crate::text::cp932_replacing(first).encode_utf16().collect();
        if name.len() >= 1024 {
            return Err(Error::new(vm.pc, "系统任务资源名称超过转换缓冲区范围"));
        }
        let mut name = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置系统任务资源服务"))?
            .resolve_utf16(b"export", &name)?;
        name.truncate(
            name.iter()
                .position(|&unit| unit == 0)
                .unwrap_or(name.len()),
        );
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "系统任务资源名称超过字段容量"));
        }
        let text = format_string(vm, read::u32_at(arguments, 8)?, 0)?;
        let text = &text[..text
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(text.len())];
        if text.len() >= 260 {
            return Err(Error::new(vm.pc, "系统任务文字超过字段容量"));
        }
        let mut record = [0; 780];
        for (bytes, unit) in record.chunks_exact_mut(2).zip(name) {
            bytes.copy_from_slice(&unit.to_le_bytes());
        }
        record[520..520 + text.len()].copy_from_slice(text);
        self.command_target_with_buffers(
            target,
            453,
            [2, 0, 0],
            &mut CommandBuffers {
                data1: Some(&mut record),
                ..Default::default()
            },
        )?;
        Ok(NativeReturn::Continue)
    }
}
