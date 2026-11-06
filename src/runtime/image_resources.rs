use super::{ObjectFactory, ObjectHost, Resources, Runtime};
use crate::graphics::{Image, ImageFilter, ImageReference, Rect};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::{Error, Result, read};

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
    fn load_image_resource(&mut self, name: &[u8]) -> Result<Option<Image>> {
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
        if parts.len() != 1 {
            return Err(Error::new(0, "独立图像资源组合图层尚未接入"));
        }
        let services = self
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置资源服务"))?;
        let animation_name = [&name[..end], b".anm"].concat();
        if let Some(animation) = services.read(&animation_name, ResourceMode::LooseFirst)? {
            if animation.is_empty() {
                return Ok(None);
            }
            return Err(Error::new(0, "独立图像资源的 ANM 来源尚未接入"));
        }
        // 单图装载使用补全后的完整名称，分词只决定是否进入组合图层路径。
        let reference = ImageReference::parse(name)?;
        let Some(bytes) = services.read(&reference.name, ResourceMode::LooseFirst)? else {
            return Ok(None);
        };
        if bytes.is_empty() {
            return Ok(None);
        }
        crate::hg3::decode_frame(&bytes, reference.frame)?
            .map(Image::from_frame)
            .transpose()
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn image_resource_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if matches!(index, 710 | 733) {
            if arguments.len() != if index == 710 { 4 } else { 8 } {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let image = if index == 733 {
                Image::new([read::u32_at(arguments, 0)?, read::u32_at(arguments, 4)?])?
            } else {
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
                let Some(image) = self.resources.load_image_resource(&name)? else {
                    return Ok(NativeReturn::Value(0));
                };
                image
            };
            let handle = self.resources.images.insert_resource(
                Some(self.host.task_id),
                image,
                index == 710,
            )?;
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if index == 735 {
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
                .ok_or_else(|| Error::new(vm.pc, "零来源图像资源的拷贝在原入口中访问空包装"))?;
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
            self.resources
                .images
                .copy_scaled(target, source, rect(24)?, rect(4)?)?;
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
            self.resources.drawing.clear_rect(rectangle);
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
            722 | 723 | 727 | 728 | 737 => 2,
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
        let resource = self.resources.images.resource_entry(handle)?;
        let image_handle = resource.image;
        let has_source_layer = resource.has_source_layer;
        if index == 730 {
            if !has_source_layer {
                return Err(Error::new(
                    vm.pc,
                    "该图像资源操作需要来源图层，空白资源的原入口会访问空容器",
                ));
            }
            self.resources
                .images
                .get_mut(image_handle)?
                .resize_canvas([read::u32_at(arguments, 4)?, read::u32_at(arguments, 8)?])?;
            self.resources.images.rebuild_resource_geometry(handle)?;
            self.resources.images.resource_entry_mut(handle)?.updated = true;
            return Ok(NativeReturn::Continue);
        }
        if index == 726 {
            if !has_source_layer {
                return Err(Error::new(
                    vm.pc,
                    "该图像资源操作需要来源图层，空白资源的原入口会访问空容器",
                ));
            }
            let radius = [
                read::u32_at(arguments, 4)? as i32,
                read::u32_at(arguments, 8)? as i32,
            ];
            let image = self.resources.images.get_mut(image_handle)?;
            image.expand_for_blur(radius)?;
            let size = image.info().size;
            let offset = image.info().offset;
            image.blur(radius)?;
            let geometry = &mut self.resources.images.resource_entry_mut(handle)?.geometry;
            geometry.size = size;
            geometry.offset = offset;
            self.resources.images.resource_entry_mut(handle)?.updated = true;
            return Ok(NativeReturn::Continue);
        }
        if matches!(index, 713 | 723 | 727 | 737) {
            if index != 737 && !has_source_layer {
                return Err(Error::new(
                    vm.pc,
                    "该图像资源操作需要来源图层，空白资源的原入口会访问空容器",
                ));
            }
            let image = self.resources.images.get_mut(image_handle)?;
            match index {
                713 => image.adjust_rgb_percent(rgb),
                723 => {
                    image.rotate_right_angle(read::u32_at(arguments, 4)? as i32)?;
                }
                727 => image.flip(read::u32_at(arguments, 4)? != 0),
                737 => filter.apply(image),
                _ => unreachable!(),
            }
            if matches!(index, 723 | 727) {
                self.resources.images.rebuild_resource_geometry(handle)?;
            }
            self.resources.images.resource_entry_mut(handle)?.updated = true;
            return Ok(NativeReturn::Continue);
        }
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
                image.auxiliary = a;
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
