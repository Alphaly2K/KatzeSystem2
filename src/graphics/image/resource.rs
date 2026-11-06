use super::{Image, Images};
use crate::{Error, Result, read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResourceGeometry {
    pub size: [u32; 2],
    pub offset: [i32; 2],
    pub canvas_size: [u32; 2],
    pub base: [i32; 2],
}

impl ResourceGeometry {
    pub(crate) fn bounds(self, position: [f32; 2], base: [f32; 2]) -> crate::graphics::Rect {
        let at: [i32; 2] = std::array::from_fn(|axis| {
            super::list_coordinate(position[axis])
                .wrapping_sub(super::list_coordinate(base[axis]))
                .wrapping_add(self.offset[axis])
        });
        crate::graphics::Rect::from_xywh(at[0], at[1], self.size[0] as i32, self.size[1] as i32)
    }

    fn from_image(image: &Image) -> Self {
        let info = image.info();
        Self {
            size: info.size,
            offset: info.offset,
            canvas_size: std::array::from_fn(|axis| {
                if info.canvas_size[axis] == 0 {
                    info.size[axis]
                } else {
                    info.canvas_size[axis]
                }
            }),
            base: info.base,
        }
    }
}

pub(crate) struct ImageResource {
    pub image: u32,
    owner: Option<u32>,
    references: usize,
    pub has_source_layer: bool,
    pub updated: bool,
    pub geometry: ResourceGeometry,
}

impl Images {
    /// 借用独立资源当前的图像；资源句柄与普通包装和像素句柄分别检查类型。
    pub fn resource(&self, handle: u32) -> Result<&Image> {
        self.get(self.resource_entry(handle)?.image)
    }

    pub(crate) fn resource_entry(&self, handle: u32) -> Result<&ImageResource> {
        self.resources
            .get(&handle)
            .ok_or_else(|| Error::new(0, "独立图像资源句柄无效"))
    }

    pub(crate) fn resource_entry_mut(&mut self, handle: u32) -> Result<&mut ImageResource> {
        self.resources
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "独立图像资源句柄无效"))
    }

    pub(crate) fn insert_resource(
        &mut self,
        owner: Option<u32>,
        image: Image,
        has_source_layer: bool,
    ) -> Result<u32> {
        if self.resources.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "独立图像资源数量超过上限"));
        }
        let handle = self.handles.allocate()?;
        let has_alpha = has_source_layer.then_some(image.info().reserved != 0);
        let geometry = ResourceGeometry::from_image(&image);
        let image = self.insert_unowned(image, has_alpha)?;
        self.resources.insert(
            handle,
            ImageResource {
                image,
                owner,
                references: 1,
                has_source_layer,
                updated: !has_source_layer,
                geometry,
            },
        );
        Ok(handle)
    }

    /// 为独立使用者增加持有；所属任务释放后资源继续有效。
    pub fn retain_resource(&mut self, handle: u32) -> Result<()> {
        let resource = self.resource_entry_mut(handle)?;
        resource.references = resource
            .references
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "独立图像资源引用超过上限"))?;
        Ok(())
    }

    /// 释放独立持有，保留任务登记；零句柄跳过。
    pub fn release_resource_reference(&mut self, handle: u32) -> Result<()> {
        self.release_resource(handle, None)
    }

    pub(crate) fn release_resource(&mut self, handle: u32, owner: Option<u32>) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let resource = self.resource_entry_mut(handle)?;
        if owner.is_some() && resource.owner == owner {
            resource.owner = None;
        }
        resource.references -= 1;
        if resource.references == 0 {
            let resource = self.resources.remove(&handle).unwrap();
            self.release(resource.image)?;
        }
        Ok(())
    }

    pub(crate) fn release_resources(&mut self, owner: u32) -> Result<()> {
        let handles: Vec<_> = self
            .resources
            .iter()
            .filter_map(|(&handle, resource)| (resource.owner == Some(owner)).then_some(handle))
            .collect();
        for handle in handles {
            self.release_resource(handle, Some(owner))?;
        }
        Ok(())
    }

    /// 读取共享更新标记；clear 同时清零，后续使用者会看到清零后的值。
    pub fn resource_updated(&mut self, handle: u32, clear: bool) -> Result<bool> {
        let resource = self.resource_entry_mut(handle)?;
        let updated = resource.updated;
        if clear {
            resource.updated = false;
        }
        Ok(updated)
    }

    pub(crate) fn rebuild_resource_geometry(&mut self, handle: u32) -> Result<()> {
        let geometry = ResourceGeometry::from_image(self.resource(handle)?);
        self.resource_entry_mut(handle)?.geometry = geometry;
        Ok(())
    }
}
