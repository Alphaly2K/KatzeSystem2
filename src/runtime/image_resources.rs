use super::{ObjectFactory, ObjectHost, Resources, Runtime};
use crate::graphics::{Image, ImageReference};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::{Error, Result, read};

pub(super) struct ImageResource {
    image: u32,
    owner: Option<u32>,
    references: usize,
}

impl<F: ObjectFactory> Runtime<F> {
    /// 借用独立图像资源当前的解码图像；句柄属于普通 710／733 资源表。
    pub fn image_resource(&self, handle: u32) -> Result<&Image> {
        let resource = self
            .resources
            .image_resources
            .get(&handle)
            .ok_or_else(|| Error::new(0, "独立图像资源句柄无效"))?;
        self.resources.images.get(resource.image)
    }

    /// 增加独立使用者的持有，所属任务释放后资源继续存活。
    pub fn retain_image_resource(&mut self, handle: u32) -> Result<()> {
        let resource = self
            .resources
            .image_resources
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "独立图像资源句柄无效"))?;
        resource.references = resource
            .references
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "独立图像资源引用超过上限"))?;
        Ok(())
    }

    /// 释放独立使用者的持有，保留所属任务登记；零句柄跳过。
    pub fn release_image_resource_reference(&mut self, handle: u32) -> Result<()> {
        self.resources.release_image_resource(handle, None)
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

    fn release_image_resource(&mut self, handle: u32, owner: Option<u32>) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let resource = self
            .image_resources
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "独立图像资源句柄无效"))?;
        if owner.is_some() && resource.owner == owner {
            resource.owner = None;
        }
        resource.references -= 1;
        if resource.references == 0 {
            let resource = self.image_resources.remove(&handle).unwrap();
            self.images.release(resource.image)?;
        }
        Ok(())
    }

    pub(super) fn release_image_resources(&mut self, owner: u32) -> Result<()> {
        let handles: Vec<_> = self
            .image_resources
            .iter()
            .filter_map(|(&handle, resource)| (resource.owner == Some(owner)).then_some(handle))
            .collect();
        for handle in handles {
            self.release_image_resource(handle, Some(owner))?;
        }
        Ok(())
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
            if self.resources.image_resources.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "独立图像资源数量超过上限"));
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
            let handle = self.host.handle_allocator().allocate_at(vm.pc)?;
            let image = self.resources.images.insert_unowned(image)?;
            self.resources.image_resources.insert(
                handle,
                ImageResource {
                    image,
                    owner: Some(self.host.task_id),
                    references: 1,
                },
            );
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if handle == 0 {
            return Ok(if matches!(index, 717 | 724) {
                NativeReturn::Value(1)
            } else {
                NativeReturn::Continue
            });
        }
        let count = match index {
            711 => 1,
            712 | 714 | 715 | 717 => 3,
            724 => 7,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if index == 711 {
            self.resources
                .release_image_resource(handle, Some(self.host.task_id))?;
            return Ok(NativeReturn::Continue);
        }
        let resource = self
            .resources
            .image_resources
            .get(&handle)
            .ok_or_else(|| Error::new(vm.pc, "独立图像资源句柄无效"))?;
        let image = self.resources.images.get(resource.image)?;
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
