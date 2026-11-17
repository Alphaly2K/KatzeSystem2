use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn text_window_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            123 | 126 | 134 | 138 => 3,
            137 | 599 => 6,
            418 => 4,
            124 | 127 | 128 | 131 | 133 | 135 | 136 | 139 | 456 | 557 | 585 => 2,
            125 | 129 | 130 | 132 | 391 => 1,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        match index {
            123 | 418 => {
                let mut record = vec![0; 0x468];
                record[..96].copy_from_slice(&vm.read(read::u32_at(arguments, 4)?, 96)?);
                for (slot, offset) in [0x60, 0x264]
                    .into_iter()
                    .take(if index == 418 { 2 } else { 1 })
                    .enumerate()
                {
                    let source = vm.read_string(read::u32_at(arguments, 8 + slot * 4)?)?;
                    let codes = crate::message::window::character_set(&source);
                    if codes.len() >= 256 {
                        return Err(Error::new(vm.pc, "文本窗口禁则字符表超过容量"));
                    }
                    record[offset..offset + 4].copy_from_slice(&(codes.len() as u32).to_le_bytes());
                    for (i, code) in codes.into_iter().enumerate() {
                        record[offset + 4 + i * 2..offset + 6 + i * 2]
                            .copy_from_slice(&code.to_le_bytes());
                    }
                }
                self.command_target_with_buffers(
                    target,
                    102,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut record),
                        ..Default::default()
                    },
                )?;
            }
            124 => {
                let mut text = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                text.push(0);
                self.command_target_with_buffers(
                    target,
                    104,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut text),
                        ..Default::default()
                    },
                )?;
            }
            125 => {
                self.command_target(target, 105, [0; 3])?;
            }
            126 => {
                self.command_target(
                    target,
                    107,
                    [read::u32_at(arguments, 4)?, read::u32_at(arguments, 8)?, 0],
                )?;
            }
            127 | 128 | 131 | 133 => {
                let code = match index {
                    127 => 108,
                    128 => 106,
                    131 => 111,
                    133 => 114,
                    _ => unreachable!(),
                };
                self.command_target(target, code, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            129 => {
                let supported = self
                    .tree
                    .find_reachable(self.host.task_id, target)
                    .is_some_and(|node| node.kind == super::kind::TEXT_WINDOW);
                let deleted = self
                    .objects
                    .get(&target)
                    .is_some_and(|object| object.control().deletion_requested != 0);
                if !supported || deleted {
                    return Err(Error::new(
                        vm.pc,
                        "文本窗口完成查询目标不可达、已删除或类型未接入",
                    ));
                }
                return Ok(NativeReturn::Value(
                    self.command_target(target, 109, [0; 3])?,
                ));
            }
            130 | 132 => {
                self.command_target(target, if index == 130 { 110 } else { 113 }, [0; 3])?;
            }
            134 => {
                let text = vm.read_string(read::u32_at(arguments, 8)?)?;
                let codes = crate::message::window::character_set(&text);
                if codes.len() > read::MAX_ITEMS {
                    return Err(Error::new(vm.pc, "文本窗口字符表超过容量"));
                }
                let mut bytes: Vec<_> = codes
                    .into_iter()
                    .chain(std::iter::once(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                self.command_target_with_buffers(
                    target,
                    115,
                    [read::u32_at(arguments, 4)?, 0, 0],
                    &mut CommandBuffers {
                        data1: Some(&mut bytes),
                        ..Default::default()
                    },
                )?;
            }
            135 | 139 | 456 | 585 => {
                self.command_target(
                    target,
                    match index {
                        135 => 116,
                        139 => 112,
                        456 => 121,
                        585 => 123,
                        _ => unreachable!(),
                    },
                    [0, read::u32_at(arguments, 4)?, 0],
                )?;
            }
            136 => {
                let handle = read::u32_at(arguments, 4)?;
                if handle != 0 {
                    let pixels = self.resources.images.pixel_handle(handle)?;
                    self.command_target(target, 117, [0, pixels, 0])?;
                }
            }
            137 => {
                let mut bytes = [0; 20];
                bytes[..2].copy_from_slice(&(read::u32_at(arguments, 4)? as u16).to_le_bytes());
                bytes[4..].copy_from_slice(&arguments[8..24]);
                self.command_target_with_buffers(
                    target,
                    118,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut bytes),
                        ..Default::default()
                    },
                )?;
            }
            138 => {
                self.command_target(
                    target,
                    119,
                    [read::u32_at(arguments, 4)?, read::u32_at(arguments, 8)?, 0],
                )?;
            }
            391 => {
                let mut bytes = [0; 4];
                self.command_target_with_buffers(
                    target,
                    120,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut bytes),
                        ..Default::default()
                    },
                )?;
                return Ok(NativeReturn::Value(u32::from_le_bytes(bytes)));
            }
            599 => {
                let mut bytes = arguments[4..24].to_vec();
                self.command_target_with_buffers(
                    target,
                    124,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut bytes),
                        ..Default::default()
                    },
                )?;
            }
            557 => {
                self.command_target(target, 122, [0, read::u32_at(arguments, 4)?, 0])?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
