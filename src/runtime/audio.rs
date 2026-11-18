use super::{ObjectFactory, ObjectHost};
use crate::audio::parse_loop_table;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn control_sound_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let mut record = if index == 602 {
            read::range(arguments, 4, 16)?.to_vec()
        } else {
            Vec::new()
        };
        let sound = self.command_target(0, 35, [0; 3])?;
        let channel = read::u32_at(arguments, 0)?;
        let mut data = [channel, 0, 0];
        match index {
            157 | 161 | 164 => data[1] = read::u32_at(arguments, 4)?,
            163 | 166 => {
                let low = u16::from_le_bytes(read::range(arguments, 4, 2)?.try_into().unwrap());
                let high = u16::from_le_bytes(read::range(arguments, 8, 2)?.try_into().unwrap());
                data[1] = u32::from(low) | (u32::from(high) << 16);
            }
            167 | 168 | 347 | 348 => {
                record.extend_from_slice(read::range(
                    arguments,
                    4,
                    match index {
                        167 => 8,
                        348 => 4,
                        _ => 12,
                    },
                )?);
            }
            349 => record.resize(4, 0xcc),
            588 => record.resize(4, 0),
            903 => {
                record = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                record.truncate(terminated(&record).len());
                if record.len() >= 1024 {
                    return Err(Error::new(vm.pc, "声音 TAG 名称超过格式缓冲区容量"));
                }
                record.push(0);
            }
            _ => {}
        }
        let query = matches!(index, 160 | 162 | 165 | 349 | 569 | 587 | 588 | 903);
        if query {
            let supported = self
                .tree
                .find_reachable(self.host.task_id, sound)
                .is_some_and(|node| node.kind == super::kind::SOUND);
            let deleted = self
                .objects
                .get(&sound)
                .is_some_and(|object| object.control().deletion_requested != 0)
                || self.running.get(&sound).is_some_and(|task| task.deleted);
            if !supported || deleted {
                return Err(Error::new(vm.pc, "声音查询目标不可达、已删除或类型未接入"));
            }
            if index == 587
                && channel >= 64
                && self
                    .objects
                    .get(&sound)
                    .is_none_or(|object| object.audio_enabled())
            {
                return Err(Error::new(vm.pc, "声音完成遍数查询的通道未定义返回值"));
            }
        }
        let output = self.command_target_with_buffers(
            sound,
            match index {
                156..=168 => 182 + index - 156,
                347..=349 => 196 + index - 347,
                569 => 201,
                587 => 202,
                588 => 203,
                602 => 204,
                603 => 205,
                903 => 215,
                _ => unreachable!(),
            },
            data,
            &mut CommandBuffers {
                data1: (!record.is_empty()).then_some(record.as_mut_slice()),
                ..Default::default()
            },
        )?;
        if index == 349 {
            // 该查询写入 data1 的临时记录，普通命令输出槽不是返回来源。
            let value = u32::from_le_bytes(record[..4].try_into().unwrap());
            if value == 0xcccccccc {
                return Err(Error::new(vm.pc, "声音渐变查询未写入结果缓冲区"));
            }
            return Ok(NativeReturn::Value(value));
        }
        Ok(if query {
            NativeReturn::Value(output)
        } else {
            NativeReturn::Continue
        })
    }

    pub(super) fn load_sound_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 1075 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let sound = self.command_target(0, 35, [0; 3])?;
            let expand = |address| -> Result<Vec<u8>> {
                let mut text = if vm.read(address, 1).is_ok() {
                    format_string(vm, address, 0)?
                } else {
                    Vec::new()
                };
                text.truncate(terminated(&text).len());
                if text.len() >= 2048 {
                    return Err(Error::new(vm.pc, "语音合成配置超过格式缓冲区容量"));
                }
                text.push(0);
                Ok(text)
            };
            let mut first = expand(read::u32_at(arguments, 0)?)?;
            let mut second = expand(read::u32_at(arguments, 4)?)?;
            self.command_target_with_buffers(
                sound,
                228,
                [0; 3],
                &mut CommandBuffers {
                    data0: Some(&mut first),
                    data1: Some(&mut second),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Continue);
        }
        let arguments = read::range(
            arguments,
            0,
            match index {
                1070 => 12,
                155 | 1071 => 16,
                936 | 1073 => 24,
                _ => 20,
            },
        )?;
        let sound = self.command_target(0, 35, [0; 3])?;
        let name = format_string(vm, read::u32_at(arguments, 8)?, 0)?;
        let name = terminated(&name);
        if matches!(index, 1070..=1073) {
            return self.load_speech_native(index, sound, arguments, name, vm.pc);
        }
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
        let offset = if matches!(index, 155 | 935) { 0 } else { 4 };
        record[offset..offset + 4].copy_from_slice(&channel.to_le_bytes());
        record[12..12 + name.len()].copy_from_slice(name);
        record[0x110..0x114].copy_from_slice(&u32::from(channel & 0xffff_0000 == 0).to_le_bytes());
        let (selector, frames, base) = if matches!(index, 155 | 935) {
            (
                read::u32_at(arguments, 12)?,
                if index == 155 {
                    0
                } else {
                    read::u32_at(arguments, 16)?
                },
                180,
            )
        } else {
            (
                read::u32_at(arguments, 16)?,
                if index == 604 {
                    0
                } else {
                    read::u32_at(arguments, 20)?
                },
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

    fn load_speech_native(
        &mut self,
        index: u32,
        sound: u32,
        arguments: &[u8],
        text: &[u8],
        pc: usize,
    ) -> Result<NativeReturn> {
        if text.len() >= 2048 {
            return Err(Error::new(pc, "语音合成文字超过格式缓冲区容量"));
        }
        if !self.require_reachable(sound) {
            return Err(Error::new(pc, "语音合成目标不可达"));
        }
        let grouped = matches!(index, 1071 | 1073);
        let code = if !grouped {
            225
        } else if read::u32_at(arguments, 12)? != 0 {
            226
        } else {
            227
        };
        let frames = match index {
            1072 => read::u32_at(arguments, 16)?,
            1073 => read::u32_at(arguments, 20)?,
            _ => 0,
        };
        // 文本紧随固定字段，以 VM 字节传递，无需在记录中保存宿主指针。
        let mut record = vec![0; 0x120];
        let offset = if grouped { 4 } else { 0 };
        record[offset..offset + 4].copy_from_slice(&arguments[..4]);
        record[0x114..0x118]
            .copy_from_slice(&((u64::from(frames) * 1000 / 60) as u32).to_le_bytes());
        record.extend_from_slice(text);
        record.push(0);
        let output = self.command_target_with_buffers(
            sound,
            code,
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
        let slot = read::u32_at(arguments, 0)?;
        let name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
        let name = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置声音资源服务"))?
            .resolve_slot(slot, &name)?;
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
                .resolve_slot(slot, &point.name)?;
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
