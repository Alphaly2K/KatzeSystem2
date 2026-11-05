use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn graphics_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 542 {
            let drawing = &mut self.resources.drawing;
            if drawing.mode == 0 && drawing.enabled {
                if let Some(target) = drawing.target.as_mut() {
                    let settings = self
                        .host
                        .system_settings
                        .as_ref()
                        .ok_or_else(|| Error::new(vm.pc, "宿主未配置画布面块区域"))?;
                    target.prepare_tiles(&settings.texture_areas, settings.texture_16bit_row)?;
                }
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 739 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let addresses = [
                u32::from_le_bytes(arguments[..4].try_into().unwrap()),
                u32::from_le_bytes(arguments[4..].try_into().unwrap()),
            ];
            let mapping = self.resources.input_coordinates;
            // 两个输出可以重叠；每次转换都读取前一次写回后的存储。
            for address in [addresses[1], addresses[0]] {
                vm.write(address, &[0; 4])?;
            }
            if self.resources.drawing.mode == 0 && self.resources.drawing.enabled {
                for (address, value) in addresses.iter().zip(mapping.truncated_device_offset()) {
                    vm.write(*address, &value.to_le_bytes())?;
                }
            }
            for address in addresses {
                let value = u32::from_le_bytes(vm.read(address, 4)?.try_into().unwrap());
                vm.write(address, &value.wrapping_mul(2).to_le_bytes())?;
            }
            for (axis, address) in addresses.into_iter().enumerate() {
                let value = i32::from_le_bytes(vm.read(address, 4)?.try_into().unwrap());
                vm.write(address, &mapping.logical_axis(axis, value).to_le_bytes())?;
            }
            return Ok(NativeReturn::Continue);
        }
        let count = match index {
            78 => 12,
            79 => 16,
            80 | 83 | 342 => 4,
            82 | 88 | 92 | 105 | 258 | 262 | 455 | 462 | 464 | 848 | 849 => 8,
            880 => 16,
            339 => 24,
            84 | 101 => 20,
            100 | 256 | 260 | 263 | 463 | 625 | 663 => 12,
            104 => 28,
            _ => unreachable!(),
        };
        if arguments.len() < 4 || arguments.len() > count || !arguments.len().is_multiple_of(4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let route_zero = matches!(
            index,
            104 | 105 | 256 | 258 | 260 | 262 | 263 | 339 | 342 | 455 | 462
                ..=464 | 625 | 663 | 848 | 849 | 880
        );
        if route_zero && arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = u32::from_le_bytes(arguments[..4].try_into().unwrap());
        if target == 0 && !route_zero {
            return Ok(NativeReturn::Continue);
        }
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let word = |i: usize| u32::from_le_bytes(arguments[i * 4..i * 4 + 4].try_into().unwrap());
        match index {
            78 | 79 => {
                let mut rectangle = [0; 32];
                rectangle[12..20].copy_from_slice(&arguments[4..12]);
                if index == 78 {
                    rectangle[20..24].copy_from_slice(&32u32.to_le_bytes());
                    rectangle[24..28].copy_from_slice(&1u32.to_le_bytes());
                    rectangle[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
                } else {
                    rectangle[28..32].copy_from_slice(&arguments[12..16]);
                }
                self.command_target_with_buffers(
                    target,
                    if index == 78 { 41 } else { 42 },
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut rectangle),
                        external: None,
                    },
                )?;
            }
            80 | 83 => {
                self.command_target(target, if index == 80 { 45 } else { 68 }, [0; 3])?;
            }
            82 => {
                let formatted = format_string(vm, word(1), 0)?;
                let name = &formatted[..formatted
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(formatted.len())];
                let services = self
                    .resources
                    .services
                    .as_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
                let mut name = services.resolve(b"kx2", name)?;
                name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
                name.push(0);
                self.command_target_with_buffers(
                    target,
                    70,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut name),
                        external: None,
                    },
                )?;
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
                        external: None,
                    },
                )?;
            }
            88 => {
                let mut vector = [0; 12];
                for code in [61, 49] {
                    if code == 49 {
                        vector[8..12].copy_from_slice(&arguments[4..8]);
                    }
                    self.command_target_with_buffers(
                        target,
                        code,
                        [0, 0, 1],
                        &mut CommandBuffers {
                            data0: None,
                            data1: Some(&mut vector),
                            external: None,
                        },
                    )?;
                }
            }
            100 => {
                self.command_target(target, 59, [word(2), word(1), 0])?;
            }
            92 => {
                self.command_target(target, 47, [0, word(1), 1])?;
            }
            105 | 848 | 849 => {
                let code = match index {
                    105 => 93,
                    848 => 99,
                    _ => 100,
                };
                self.command_target(target, code, [0, word(1), 0])?;
            }
            104 => {
                let values = [
                    word(1),
                    word(2),
                    word(1).wrapping_add(word(3)),
                    word(2).wrapping_add(word(4)),
                    word(5),
                    word(6),
                ];
                let mut rectangle = [0; 24];
                for (bytes, value) in rectangle.chunks_exact_mut(4).zip(values) {
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
                self.command_target_with_buffers(
                    target,
                    90,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut rectangle),
                        external: None,
                    },
                )?;
            }
            256 | 260 | 263 => {
                let code = match index {
                    256 => 285,
                    260 => 289,
                    _ => 293,
                };
                self.command_target(target, code, [word(1), word(2), 0])?;
            }
            258 => {
                self.command_target(target, 287, [word(1), 0, 0])?;
            }
            262 => {
                self.command_target(target, 292, [0, word(1), 0])?;
            }
            342 => {
                self.command_target(target, 298, [0; 3])?;
            }
            339 => {
                let mut rectangle = [0; 16];
                let values = [
                    word(2),
                    word(3),
                    word(2).wrapping_add(word(4)),
                    word(3).wrapping_add(word(5)),
                ];
                for (bytes, value) in rectangle.chunks_exact_mut(4).zip(values) {
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
                self.command_target_with_buffers(
                    target,
                    295,
                    [word(1), 0, 0],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut rectangle),
                        external: None,
                    },
                )?;
            }
            455 => {
                self.command_target(target, 302, [0, word(1), 0])?;
            }
            462 => {
                let mut priority = u32::MAX.to_le_bytes();
                self.command_target_with_buffers(
                    target,
                    304,
                    [word(1), 0, 0],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut priority),
                        external: None,
                    },
                )?;
                return Ok(NativeReturn::Value(u32::from_le_bytes(priority)));
            }
            463 | 625 | 663 => {
                let code = match index {
                    463 => 305,
                    625 => 301,
                    _ => 314,
                };
                self.command_target(target, code, [word(1), word(2), 0])?;
            }
            464 => {
                self.command_target(target, 306, [word(1), 0, 0])?;
            }
            880 => {
                let mut values = [0; 8];
                values.copy_from_slice(&arguments[8..16]);
                self.command_target_with_buffers(
                    target,
                    318,
                    [word(1), 0, 0],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut values),
                        external: None,
                    },
                )?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
