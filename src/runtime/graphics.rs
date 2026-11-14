use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};
mod echo;

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn graphics_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if matches!(index, 398 | 399) {
            return self.echo_native(index, arguments, vm);
        }
        if matches!(index, 858 | 859) {
            let count = if index == 858 { 4 } else { 12 };
            if arguments.len() != count {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let first = read::u32_at(arguments, 0)?;
            if index == 858 {
                let drawing = self.resources.compositor.drawing();
                if drawing.mode == 0 && drawing.enabled {
                    crate::kcs::Host::reserve_auxiliary_targets(self, first)?;
                }
            } else {
                let range = [
                    f32::from_bits(read::u32_at(arguments, 4)?),
                    f32::from_bits(read::u32_at(arguments, 8)?),
                ];
                self.resources
                    .render_auxiliary(first, self.host.surface_size, range)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 113 {
            if !arguments.is_empty() {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let Some(image) = self
                .resources
                .capture_image(self.host.surface_size, [f32::NEG_INFINITY, f32::INFINITY])?
            else {
                return Ok(NativeReturn::Value(0));
            };
            let handle = self
                .resources
                .images
                .insert(self.host.task_id, image, None)?;
            return Ok(NativeReturn::Value(handle));
        }
        if index == 568 {
            let range = [
                f32::from_bits(read::u32_at(arguments, 0)?),
                f32::from_bits(read::u32_at(arguments, 4)?),
            ];
            self.resources
                .render_offscreen(self.host.surface_size, range)?;
            return Ok(NativeReturn::Continue);
        }
        if index == 571 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let target = read::u32_at(arguments, 0)?;
            let image = read::u32_at(arguments, 4)?;
            let pixels = if image == 0 {
                0
            } else {
                self.resources.images.pixel_handle(image)?
            };
            self.command_target(target, 451, [0, pixels, 0])?;
            return Ok(NativeReturn::Continue);
        }
        if index == 542 {
            let drawing = &mut self.resources.compositor.drawing;
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
            if self.resources.compositor.drawing.mode == 0
                && self.resources.compositor.drawing.enabled
            {
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
            80
            | 83
            | 89
            | 91
            | 98
            | 99
            | 107
            | 261
            | 340..=342
            | 570
            | 572
            | 1065
            | 1067
            | 1069 => 4,
            82 | 88 | 92 | 94 | 95 | 97 | 102 | 103 | 105 | 106 | 258 | 262 | 402 | 455 | 462
            | 464 | 466 | 534 | 653 | 848 | 849 | 1066 | 1068 => 8,
            880 => 16,
            339 => 24,
            84 | 87 | 93 | 101 | 345 | 862 => 20,
            85 | 86 => 16,
            96 | 100 | 256 | 260 | 263 | 343 | 344 | 463 | 625 | 663 | 879 | 881 => 12,
            104 => 28,
            732 => 52,
            _ => unreachable!(),
        };
        if arguments.len() < 4 || arguments.len() > count || !arguments.len().is_multiple_of(4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let route_zero = matches!(
            index,
            104 | 105 | 106 | 107 | 256 | 258 | 260 | 261 | 262 | 263 | 339..=342 | 402 | 455 | 732 | 462
                ..=464 | 534 | 570 | 572 | 625 | 653 | 663 | 848 | 849 | 862 | 879..=881 | 1065..=1069
        );
        if route_zero && arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = u32::from_le_bytes(arguments[..4].try_into().unwrap());
        if target == 0 && !route_zero {
            return Ok(if matches!(index, 89 | 98) {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let word = |i: usize| u32::from_le_bytes(arguments[i * 4..i * 4 + 4].try_into().unwrap());
        match index {
            862 => {
                let mut rectangle = arguments[4..20].to_vec();
                self.command_target_with_buffers(
                    target,
                    316,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut rectangle),
                        ..Default::default()
                    },
                )?;
            }
            732 => {
                let mut record = [0; 60];
                record[12..16].copy_from_slice(&arguments[8..12]);
                record[16..44].copy_from_slice(&arguments[12..40]);
                record[44..48].copy_from_slice(&word(10).min(255).to_le_bytes());
                record[48..56].copy_from_slice(&arguments[44..52]);
                self.command_target_with_buffers(
                    target,
                    288,
                    [word(1), 0, 0],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut record),
                        external: None,
                    },
                )?;
            }
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
            80 | 83 | 91 | 99 | 570 | 572 | 1065 | 1067 | 1069 => {
                let code = match index {
                    80 => 45,
                    83 => 68,
                    91 => 54,
                    99 => 91,
                    570 => 448,
                    572 => 450,
                    1065 => 590,
                    _ => 592,
                };
                self.command_target(target, code, [0; 3])?;
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
            84 | 93 | 101 => {
                let mut vector = [0; 12];
                vector.copy_from_slice(&arguments[4..16]);
                let code = match index {
                    84 => 51,
                    93 => 55,
                    _ => 66,
                };
                self.command_target_with_buffers(
                    target,
                    code,
                    [word(4), 0, u32::from(index == 93)],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut vector),
                        external: None,
                    },
                )?;
            }
            85 | 88 => {
                let mut vector = [0; 12];
                let source = if index == 85 { word(3) } else { 0 };
                for code in [61, 49] {
                    if code == 49 {
                        if index == 85 {
                            vector[..8].copy_from_slice(&arguments[4..12]);
                        } else {
                            vector[8..12].copy_from_slice(&arguments[4..8]);
                        }
                    }
                    self.command_target_with_buffers(
                        target,
                        code,
                        [source, 0, 1],
                        &mut CommandBuffers {
                            data0: None,
                            data1: Some(&mut vector),
                            external: None,
                        },
                    )?;
                }
            }
            86 | 87 | 89 => {
                let mut vector = [0; 12];
                let source = match index {
                    86 => word(3),
                    87 => word(4),
                    _ => 0,
                };
                self.command_target_with_buffers(
                    target,
                    if index == 87 { 62 } else { 61 },
                    [source, 0, 1],
                    &mut CommandBuffers {
                        data1: Some(&mut vector),
                        ..CommandBuffers::default()
                    },
                )?;
                if index == 89 {
                    return Ok(NativeReturn::Value(u32::from_le_bytes(
                        vector[8..12].try_into().unwrap(),
                    )));
                }
                let axes = if index == 86 { 2 } else { 3 };
                for axis in 0..axes {
                    vm.read(word(axis + 1), 4)?;
                }
                for axis in 0..axes {
                    vm.write(word(axis + 1), &vector[axis * 4..axis * 4 + 4])?;
                }
            }
            94 => {
                let alpha = (word(1) as i32).clamp(0, 255) as u32;
                self.command_target(target, 63, [0, alpha, 1])?;
            }
            95 => {
                self.command_target(target, 90, [self.host.task_id, word(1), 0])?;
            }
            96 => {
                let mut states = vm.read(word(2), 48)?.to_vec();
                self.command_target_with_buffers(
                    target,
                    72,
                    [0, word(1), 0],
                    &mut CommandBuffers {
                        data0: Some(&mut states),
                        ..CommandBuffers::default()
                    },
                )?;
            }
            98 => {
                return Ok(NativeReturn::Value(
                    self.command_target(target, 74, [0; 3])?,
                ));
            }
            107 => {
                let input = self.command_target(0, 34, [0; 3])?;
                return Ok(NativeReturn::Value(self.command_target(
                    input,
                    134,
                    [target, 0, 0],
                )?));
            }
            343 => {
                let addresses = [word(1), word(2)];
                let mut width = vm.read(addresses[0], 4)?.to_vec();
                let mut height = vm.read(addresses[1], 4)?.to_vec();
                let result = self.command_target_with_buffers(
                    target,
                    75,
                    [addresses[0], addresses[1], 0],
                    &mut CommandBuffers {
                        data0: Some(&mut width),
                        data1: Some(&mut height),
                        external: None,
                    },
                );
                vm.write(addresses[0], &width)?;
                vm.write(addresses[1], &height)?;
                result?;
            }
            344 => {
                let mut matrix = vm.read(word(1), 64)?.to_vec();
                let result = self.command_target_with_buffers(
                    target,
                    57,
                    [word(2), word(1), 1],
                    &mut CommandBuffers {
                        data1: Some(&mut matrix),
                        ..CommandBuffers::default()
                    },
                );
                vm.write(word(1), &matrix)?;
                result?;
            }
            345 => {
                for axis in 1..=4 {
                    vm.read(word(axis), 4)?;
                }
                let mut start = [0; 12];
                let mut end = [0; 12];
                self.command_target_with_buffers(
                    target,
                    76,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: Some(&mut start),
                        data1: Some(&mut end),
                        external: None,
                    },
                )?;
                for (address, bytes) in [word(1), word(2), word(3), word(4)].into_iter().zip([
                    &start[..4],
                    &start[4..8],
                    &end[..4],
                    &end[4..8],
                ]) {
                    vm.write(address, bytes)?;
                }
            }
            100 => {
                self.command_target(target, 59, [word(2), word(1), 0])?;
            }
            92 => {
                self.command_target(target, 47, [0, word(1), 1])?;
            }
            97 | 102 | 103 | 105 | 106 | 402 | 466 | 653 | 848 | 849 | 1066 | 1068 => {
                let code = match index {
                    97 => 73,
                    102 => 77,
                    103 => 78,
                    105 => 93,
                    106 => 94,
                    402 => 81,
                    466 => 82,
                    653 => 84,
                    848 => 99,
                    849 => 100,
                    _ => 591,
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
            261 => {
                let mut image = [0; 4];
                self.command_target_with_buffers(
                    target,
                    291,
                    [0; 3],
                    &mut CommandBuffers {
                        data1: Some(&mut image),
                        ..Default::default()
                    },
                )?;
                return Ok(NativeReturn::Value(u32::from_le_bytes(image)));
            }
            262 | 534 => {
                self.command_target(
                    target,
                    if index == 262 { 292 } else { 310 },
                    [0, word(1), 0],
                )?;
            }
            340..=342 => {
                self.command_target(target, 296 + index - 340, [0; 3])?;
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
            879 | 881 => {
                self.command_target(
                    target,
                    if index == 879 { 317 } else { 319 },
                    [word(1), word(2), 0],
                )?;
            }
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }
}
