use super::{ObjectFactory, ObjectHost, Resources, Runtime};
use crate::graphics::Image;
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result, read};

pub(super) struct ImageResource {
    image: u32,
    owner: Option<u32>,
    references: usize,
}

impl<F: ObjectFactory> Runtime<F> {
    /// 借用独立图像资源当前的解码图像；句柄属于普通 733 资源表。
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
        if index == 733 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            if self.resources.image_resources.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "独立图像资源数量超过上限"));
            }
            let image = Image::new([read::u32_at(arguments, 0)?, read::u32_at(arguments, 4)?])?;
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
