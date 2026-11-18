use super::{ObjectFactory, ObjectHost, Resources, Runtime};
use crate::graphics::{Image, ImageFilter, ImageReference, Rect, ResourceTransform};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::{Error, Result, read};

type ImageLayers = Vec<(Image, Option<bool>)>;

impl<F: ObjectFactory> Runtime<F> {
    /// 借用独立图像资源当前的解码图像；句柄属于普通 710／733 资源表。
    pub fn image_resource(&self, handle: u32) -> Result<&Image> {
        self.resources.images.resource(handle)
    }

    /// 增加独立使用者的持有，所属任务释放后资源继续存活。
    pub fn retain_image_resource(&mut self, handle: u32) -> Result<()> {
        self.resources.images.retain_resource(handle)
    }

    /// 释放独立使用者的持有，保留所属任务登记；零句柄跳过。
    pub fn release_image_resource_reference(&mut self, handle: u32) -> Result<()> {
        self.resources.images.release_resource_reference(handle)
    }

    /// 读取资源的更新标记，可同时清零；清零会影响共享这一资源的全部使用者。
    pub fn image_resource_updated(&mut self, handle: u32, clear: bool) -> Result<bool> {
        self.resources.images.resource_updated(handle, clear)
    }
}

impl Resources {
    pub(super) fn load_image_resource(
        &mut self,
        name: &[u8],
        preload: bool,
    ) -> Result<Option<ImageLayers>> {
        if name.len() >= 260 {
            return Err(Error::new(0, "图像资源名超过基线缓冲区范围"));
        }
        let end = name
            .iter()
            .rposition(|byte| matches!(byte, b'.' | b'/' | b'\\'))
            .filter(|&position| name[position] == b'.')
            .unwrap_or(name.len());
        let parts: Vec<_> = name[..end]
            .split(|&byte| byte == b',')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            return Ok(None);
        }
        let table_length = parts.iter().map(|part| part.len() + 1).sum::<usize>()
            + 1
            + usize::from(name[..end].last() == Some(&b','));
        if table_length > 256 || parts.len() > 65 {
            return Err(Error::new(0, "独立图像资源名称分量超过缓冲区上限"));
        }
        if parts.len() > 1 {
            let mut layers = Vec::new();
            for (index, part) in parts[1..].iter().enumerate() {
                let value = part[0];
                if value == b'0' {
                    continue;
                }
                let mut layer = parts[0].to_vec();
                layer.push(b'_');
                layer.resize(layer.len() + index, b'0');
                // 分量来自有符号 char 的 DWORD，字符串在第四字节处终止。
                let value = (value as i8 as i32).to_le_bytes();
                layer.extend(value[..3].iter().take_while(|&&byte| byte != 0));
                let extension = name.get(end + 1..).unwrap_or_default();
                if layer.len() + 1 + extension.len() >= 260 {
                    return Err(Error::new(0, "图层资源名超过基线缓冲区范围"));
                }
                layer.push(b'.');
                layer.extend_from_slice(extension);
                if let Some(layer) = self.decode_image_layer(&layer, &layer, 0, preload)? {
                    layers.push(layer);
                }
            }
            return Ok((!layers.is_empty()).then_some(layers));
        }
        let reference = if preload {
            ImageReference {
                name: name.to_vec(),
                frame: 0,
            }
        } else {
            ImageReference::parse(name)?
        };
        Ok(self
            .decode_image_layer(name, &reference.name, reference.frame, preload)?
            .map(|layer| vec![layer]))
    }

    fn decode_image_layer(
        &mut self,
        name: &[u8],
        image: &[u8],
        frame: i32,
        preload: bool,
    ) -> Result<Option<(Image, Option<bool>)>> {
        let animated = !preload && self.find_image_animation(name)?.is_some();
        let services = self
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置资源服务"))?;
        let loaded = self
            .images
            .decode(image, (!animated).then_some(frame), preload, services)?;
        Ok(loaded.map(|image| {
            let has_alpha = image.info().reserved != 0;
            (image, Some(has_alpha))
        }))
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn image_resource_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if matches!(index, 681 | 682) {
            if arguments.len() != 4 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let value = read::u32_at(arguments, 0)?;
            if index == 682 {
                self.resources.images.set_automatic_cache(value != 0);
            } else {
                let mut name = format_string(vm, value, 0)?;
                name.truncate(
                    name.iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(name.len()),
                );
                let name = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?
                    .resolve(b"image", &name)?;
                self.resources.load_image_resource(&name, true)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if matches!(index, 710 | 733) {
            if arguments.len() != if index == 710 { 4 } else { 8 } {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            if index == 733 {
                let handle = self.resources.images.insert_resource(
                    Some(self.host.task_id),
                    Image::new([read::u32_at(arguments, 0)?, read::u32_at(arguments, 4)?])?,
                    false,
                    None,
                )?;
                return Ok(NativeReturn::Value(handle));
            }
            let layers = {
                let address = read::u32_at(arguments, 0)?;
                if vm.read(address, 1).is_err() {
                    return Ok(NativeReturn::Value(0));
                }
                let mut name = format_string(vm, address, 0)?;
                name.truncate(
                    name.iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(name.len()),
                );
                if name.len() >= 260 {
                    return Err(Error::new(vm.pc, "图像资源名超过基线缓冲区范围"));
                }
                if self.host.unicode_text != 0 {
                    let text = std::str::from_utf8(&name)
                        .map_err(|_| Error::new(vm.pc, "图像资源名不是有效 UTF-8"))?;
                    let (encoded, _, errors) = encoding_rs::SHIFT_JIS.encode(text);
                    if errors {
                        return Err(Error::new(vm.pc, "图像资源名无法转为 CP932"));
                    }
                    name = encoded.into_owned();
                }
                crate::text::cp932(&name)?;
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
                let name = services.resolve(b"image", &name)?;
                let Some(image) = self.resources.load_image_resource(&name, false)? else {
                    return Ok(NativeReturn::Value(0));
                };
                image
            };
            let tiles = self
                .resources
                .compositor
                .drawing
                .target
                .as_ref()
                .map(|target| target.tile_size());
            let handle = self.resources.images.insert_layered_resource(
                Some(self.host.task_id),
                layers,
                tiles,
            )?;
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if index == 725 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let source = read::u32_at(arguments, 4)?;
            if handle == 0 || source == 0 {
                return Ok(NativeReturn::Value(1));
            }
            let target = self.resources.images.resource_entry(handle)?.image;
            let source = self.resources.images.resource_entry(source)?.image;
            self.resources.images.copy_attributes(target, source)?;
            return Ok(NativeReturn::Value(1));
        }
        if index == 877 {
            if arguments.len() != 4 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            return Ok(NativeReturn::Value(if handle == 0 {
                0
            } else {
                self.resources.images.resource(handle)?.alpha_mask()
            }));
        }
        if matches!(index, 734 | 735) {
            let source = if handle == 0 {
                None
            } else {
                Some(self.resources.images.resource_entry(handle)?.image)
            };
            let target = read::u32_at(arguments, 20)?;
            if target == 0 {
                return Ok(NativeReturn::Continue);
            }
            let target = self.resources.images.resource_entry(target)?.image;
            let source = source
                .ok_or_else(|| Error::new(vm.pc, "零来源图像资源的像素传输在原入口中访问空包装"))?;
            if arguments.len() != 40 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let rect = |offset| -> Result<Rect> {
                Ok(Rect::from_xywh(
                    read::u32_at(arguments, offset)? as i32,
                    read::u32_at(arguments, offset + 4)? as i32,
                    read::u32_at(arguments, offset + 8)? as i32,
                    read::u32_at(arguments, offset + 12)? as i32,
                ))
            };
            if index == 734 {
                self.resources
                    .images
                    .blend_rectangles(target, source, rect(24)?, rect(4)?)?;
            } else {
                self.resources
                    .images
                    .copy_scaled(target, source, rect(24)?, rect(4)?)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 736 && handle == 0 {
            if !matches!(arguments.len(), 20 | 24) {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let rectangle = Rect::from_xywh(
                read::u32_at(arguments, 4)? as i32,
                read::u32_at(arguments, 8)? as i32,
                read::u32_at(arguments, 12)? as i32,
                read::u32_at(arguments, 16)? as i32,
            );
            self.resources.compositor.drawing.clear_rect(rectangle);
            return Ok(NativeReturn::Continue);
        }
        if handle == 0 {
            if index == 721 {
                return Err(Error::new(vm.pc, "零图像资源的 Alpha 查询返回值未初始化"));
            }
            return Ok(if matches!(index, 717..=720 | 724) {
                NativeReturn::Value(1)
            } else {
                NativeReturn::Continue
            });
        }
        let count = match index {
            711 | 721 => 1,
            716 | 722 | 723 | 727..=729 | 737 => 2,
            713 => 4,
            712 | 714 | 715 | 717..=720 | 726 | 730 => 3,
            724 => 7,
            736 => 6,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if index == 711 {
            self.resources
                .images
                .release_resource(handle, Some(self.host.task_id))?;
            return Ok(NativeReturn::Continue);
        }
        let filter = if index == 737 {
            match read::u32_at(arguments, 4)? {
                1 => ImageFilter::Mono,
                2 => ImageFilter::Sepia,
                3 => ImageFilter::Negative,
                _ => return Ok(NativeReturn::Continue),
            }
        } else {
            ImageFilter::None
        };
        let rgb = if index == 713 {
            let rgb = [
                read::u32_at(arguments, 4)?,
                read::u32_at(arguments, 8)?,
                read::u32_at(arguments, 12)?,
            ];
            if rgb == [100; 3] {
                return Ok(NativeReturn::Continue);
            }
            rgb
        } else {
            [100; 3]
        };
        if matches!(index, 713 | 716 | 723 | 726 | 727 | 729 | 730 | 737) {
            let transform = match index {
                713 => ResourceTransform::Rgb(rgb),
                716 => ResourceTransform::Color(read::u32_at(arguments, 4)?),
                723 => ResourceTransform::Rotate(read::u32_at(arguments, 4)? as i32),
                726 => ResourceTransform::Blur([
                    read::u32_at(arguments, 4)? as i32,
                    read::u32_at(arguments, 8)? as i32,
                ]),
                727 => ResourceTransform::Flip(read::u32_at(arguments, 4)? != 0),
                729 => ResourceTransform::Alpha(read::u32_at(arguments, 4)?),
                730 => ResourceTransform::Resize([
                    read::u32_at(arguments, 4)?,
                    read::u32_at(arguments, 8)?,
                ]),
                737 => ResourceTransform::Filter(filter),
                _ => unreachable!(),
            };
            let tiles = self
                .resources
                .compositor
                .drawing
                .target
                .as_ref()
                .map(|target| target.tile_size());
            self.resources
                .images
                .transform_resource(handle, transform, tiles)?;
            return Ok(NativeReturn::Continue);
        }
        let image_handle = self.resources.images.resource_entry(handle)?.image;
        if index == 721 {
            return Ok(NativeReturn::Value(u32::from(
                self.resources.images.uses_alpha(image_handle)?,
            )));
        }
        if index == 722 {
            self.resources
                .images
                .set_uses_alpha(image_handle, read::u32_at(arguments, 4)? != 0)?;
            return Ok(NativeReturn::Continue);
        }
        if matches!(index, 718..=720 | 728 | 736) {
            let image = self.resources.images.get_mut(image_handle)?;
            let a = read::u32_at(arguments, 4)?;
            if index == 728 {
                image.alpha_mask = a;
                return Ok(NativeReturn::Continue);
            }
            let b = read::u32_at(arguments, 8)?;
            if index == 736 {
                image.fill_rect_from_bottom(
                    Rect::from_xywh(
                        a as i32,
                        b as i32,
                        read::u32_at(arguments, 12)? as i32,
                        read::u32_at(arguments, 16)? as i32,
                    ),
                    read::u32_at(arguments, 20)?,
                );
                return Ok(NativeReturn::Continue);
            }
            let info = image.info_mut();
            match index {
                718 => info.canvas_size = [a, b],
                719 => info.offset = [a as i32, b as i32],
                720 => info.base = [a as i32, b as i32],
                _ => unreachable!(),
            }
            return Ok(NativeReturn::Value(1));
        }
        let image = self.resources.images.get(image_handle)?;
        if index == 724 {
            let Some(values) = image.attribute(read::u32_at(arguments, 4)?) else {
                return Ok(NativeReturn::Value(0));
            };
            let addresses: Vec<_> = arguments[8..]
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            for &address in &addresses {
                vm.read(address, 4)?;
            }
            for (address, value) in addresses.into_iter().zip(values) {
                vm.write(address, &value.to_le_bytes())?;
            }
            return Ok(NativeReturn::Value(1));
        }
        let info = image.info();
        let values = match index {
            712 => info.size,
            714 => std::array::from_fn(|i| {
                if info.canvas_size[i] == 0 {
                    info.size[i]
                } else {
                    info.canvas_size[i]
                }
            }),
            715 => info.offset.map(|value| value as u32),
            717 => info.base.map(|value| value as u32),
            _ => unreachable!(),
        };
        let addresses = [read::u32_at(arguments, 4)?, read::u32_at(arguments, 8)?];
        for address in addresses {
            vm.read(address, 4)?;
        }
        if index != 717 {
            for address in addresses {
                vm.write(address, &[0; 4])?;
            }
        }
        for (address, value) in addresses.into_iter().zip(values) {
            vm.write(address, &value.to_le_bytes())?;
        }
        Ok(if index == 717 {
            NativeReturn::Value(1)
        } else {
            NativeReturn::Continue
        })
    }
}
