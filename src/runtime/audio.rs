use super::{ObjectFactory, ObjectHost};
use crate::audio::parse_loop_table;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn load_sound_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let arguments = read::range(arguments, 0, if index == 935 { 20 } else { 24 })?;
        let sound = self.command_target(0, 35, [0; 3])?;
        let name = format_string(vm, read::u32_at(arguments, 8)?, 0)?;
        let name = terminated(&name);
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "声音装载名称超过格式缓冲区容量"));
        }
        let name = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置声音资源服务"))?
            .resolve(b"sound", name)?;
        let name = terminated(&name);
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "声音装载名称超过记录容量"));
        }
        let mut record = [0; 0x124];
        let channel = read::u32_at(arguments, 0)?;
        let offset = if index == 935 { 0 } else { 4 };
        record[offset..offset + 4].copy_from_slice(&channel.to_le_bytes());
        record[12..12 + name.len()].copy_from_slice(name);
        record[0x110..0x114].copy_from_slice(&u32::from(channel & 0xffff_0000 == 0).to_le_bytes());
        let (selector, frames, base) = if index == 935 {
            (
                read::u32_at(arguments, 12)?,
                read::u32_at(arguments, 16)?,
                180,
            )
        } else {
            (
                read::u32_at(arguments, 16)?,
                read::u32_at(arguments, 20)?,
                if read::u32_at(arguments, 12)? == 0 {
                    208
                } else {
                    206
                },
            )
        };
        let milliseconds = (u64::from(frames) * 1000 / 60) as u32;
        record[0x114..0x118].copy_from_slice(&milliseconds.to_le_bytes());
        if !matches!(selector, 1 | 2) {
            return Err(Error::new(vm.pc, "声音装载选择器未定义返回值"));
        }
        if !self.require_reachable(sound) {
            return Err(Error::new(vm.pc, "声音装载目标不可达"));
        }
        let output = self.command_target_with_buffers(
            sound,
            base + selector - 1,
            [0; 3],
            &mut CommandBuffers {
                data1: Some(&mut record),
                ..Default::default()
            },
        )?;
        Ok(NativeReturn::Value(output))
    }

    pub(super) fn load_sound_loop_table(
        &mut self,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
        let name = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置声音资源服务"))?
            .resolve(b"sound", &name)?;
        let name = terminated(&name);
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "声音循环点表名称超过配置容量"));
        }
        let sound = self.command_target(0, 35, [0; 3])?;
        let bytes = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置声音资源服务"))?
            .read(name, ResourceMode::LooseFirst)?;
        let Some(bytes) = bytes else {
            return Ok(NativeReturn::Continue);
        };
        for point in parse_loop_table(&bytes)? {
            let name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置声音资源服务"))?
                .resolve(b"sound", &point.name)?;
            let name = terminated(&name);
            if name.len() >= 32 {
                return Err(Error::new(vm.pc, "声音循环点名称超过配置容量"));
            }
            if !self.require_reachable(sound) {
                return Err(Error::new(vm.pc, "声音循环点登记目标不可达"));
            }
            let mut record = [0; 48];
            record[..name.len()].copy_from_slice(name);
            record[32..40].copy_from_slice(&point.start.to_le_bytes());
            record[40..48].copy_from_slice(&point.end.to_le_bytes());
            self.command_target_with_buffers(
                sound,
                199,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
            )?;
        }
        Ok(NativeReturn::Continue)
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
