use super::{Image, Images};
use crate::graphics::{ImageFilter, ImageList, ImageListProperties, Rect};
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

    pub(crate) fn from_image(image: &Image) -> Self {
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

    pub(super) fn from_layers<'a>(layers: impl IntoIterator<Item = &'a Image>) -> Option<Self> {
        let mut geometry: Option<Self> = None;
        let mut bounds: Option<Rect> = None;
        for image in layers {
            let next = Self::from_image(image);
            let merged = geometry.get_or_insert(next);
            for axis in 0..2 {
                merged.canvas_size[axis] = (merged.canvas_size[axis] as i32)
                    .max(next.canvas_size[axis] as i32)
                    .max(0) as u32;
            }
            let rect = next.bounds([0.; 2], [0.; 2]);
            if rect.left < rect.right && rect.top < rect.bottom {
                bounds = Some(bounds.map_or(rect, |old| Rect {
                    left: old.left.min(rect.left),
                    top: old.top.min(rect.top),
                    right: old.right.max(rect.right),
                    bottom: old.bottom.max(rect.bottom),
                }));
            }
        }
        geometry.map(|mut geometry| {
            geometry.size = bounds.map_or([0; 2], |bounds| {
                [
                    bounds.right.wrapping_sub(bounds.left) as u32,
                    bounds.bottom.wrapping_sub(bounds.top) as u32,
                ]
            });
            geometry.offset = bounds.map_or([0; 2], |bounds| [bounds.left, bounds.top]);
            if geometry.canvas_size.contains(&0) {
                geometry.canvas_size = [0; 2];
            }
            geometry
        })
    }

    pub(super) fn apply(self, image: &mut Image) {
        image.info.offset = self.offset;
        image.info.canvas_size = self.canvas_size;
        image.info.base = self.base;
    }

    pub(super) fn layer_properties(self, image: &Image) -> ImageListProperties {
        let base = image.info.base;
        ImageListProperties {
            position: std::array::from_fn(|axis| {
                base[axis].wrapping_sub(self.offset[axis]) as u32 as f32
            }),
            base_point: base.map(|value| value as f32),
            ..Default::default()
        }
    }
}

pub(crate) enum ResourceTransform {
    Color(u32),
    Rgb([u32; 3]),
    Alpha(u32),
    Filter(ImageFilter),
    Rotate(i32),
    Flip(bool),
    Resize([u32; 2]),
    Blur([i32; 2]),
}

pub(crate) struct ImageResource {
    pub image: u32,
    owner: Option<u32>,
    references: usize,
    sources: Vec<u32>,
    composition: Option<ImageList>,
    pub updated: bool,
    pub geometry: ResourceGeometry,
}

impl ImageResource {
    fn rebuild(&mut self, images: &mut Images, tiles: Option<[u32; 2]>) -> Result<()> {
        let geometry = ResourceGeometry::from_layers(
            self.sources
                .iter()
                .map(|handle| images.get(*handle).unwrap()),
        )
        .ok_or_else(|| Error::new(0, "图像资源没有来源图层"))?;
        if self.sources.len() == 1 {
            self.geometry = geometry;
            self.image = self.sources[0];
            return Ok(());
        }
        let tiles = tiles.unwrap_or([geometry.size[0].max(geometry.size[1]).max(1); 2]);
        let mut list = ImageList::new_tiled_for(None, geometry.size, tiles, images)?;
        let result = (|| {
            for (index, &source) in self.sources.iter().enumerate() {
                let properties = geometry.layer_properties(images.get(source)?);
                list.set_layer(index as u32, source, properties, images)?;
                images.copy_attributes(list.image(), source)?;
            }
            list.update(images)?;
            geometry.apply(images.get_mut(list.image())?);
            Ok(())
        })();
        if let Err(error) = result {
            list.release(images)?;
            return Err(error);
        }
        if let Some(mut previous) = self.composition.take() {
            previous.release(images)?;
        }
        self.geometry = geometry;
        self.image = list.image();
        self.composition = Some(list);
        Ok(())
    }

    fn release(mut self, images: &mut Images) -> Result<()> {
        if let Some(mut list) = self.composition.take() {
            list.release(images)?;
        } else if self.sources.is_empty() {
            images.release(self.image)?;
        }
        for source in self.sources {
            images.release(source)?;
        }
        Ok(())
    }
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
        has_alpha: Option<bool>,
    ) -> Result<u32> {
        if self.resources.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "独立图像资源数量超过上限"));
        }
        let handle = self.handles.allocate()?;
        let geometry = if has_source_layer {
            ResourceGeometry::from_layers(std::iter::once(&image)).unwrap()
        } else {
            ResourceGeometry::from_image(&image)
        };
        let image = self.insert_unowned(image, has_alpha)?;
        self.resources.insert(
            handle,
            ImageResource {
                image,
                owner,
                references: 1,
                sources: if has_source_layer {
                    vec![image]
                } else {
                    Vec::new()
                },
                composition: None,
                updated: !has_source_layer,
                geometry,
            },
        );
        Ok(handle)
    }

    pub(crate) fn insert_layered_resource(
        &mut self,
        owner: Option<u32>,
        layers: Vec<(Image, Option<bool>)>,
        tiles: Option<[u32; 2]>,
    ) -> Result<u32> {
        if layers.len() == 1 {
            let (image, alpha) = layers.into_iter().next().unwrap();
            return self.insert_resource(owner, image, true, alpha);
        }
        if layers.is_empty() || self.resources.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "独立图像资源来源数量无效或资源数量超过上限"));
        }
        let handle = self.handles.allocate()?;
        let mut resource = ImageResource {
            image: 0,
            owner,
            references: 1,
            sources: Vec::new(),
            composition: None,
            updated: false,
            geometry: ResourceGeometry::from_image(&layers[0].0),
        };
        let result = (|| {
            for (image, alpha) in layers {
                resource.sources.push(self.insert_unowned(image, alpha)?);
            }
            resource.rebuild(self, tiles)
        })();
        if let Err(error) = result {
            resource.release(self)?;
            return Err(error);
        }
        self.resources.insert(handle, resource);
        Ok(handle)
    }

    pub(crate) fn transform_resource(
        &mut self,
        handle: u32,
        transform: ResourceTransform,
        tiles: Option<[u32; 2]>,
    ) -> Result<()> {
        self.with_resource(handle, |resource, images| {
            if resource.sources.is_empty() {
                if let ResourceTransform::Filter(filter) = transform {
                    filter.apply(images.get_mut(resource.image)?);
                    resource.updated = true;
                    return Ok(());
                }
                return Err(Error::new(
                    0,
                    "该图像资源操作需要来源图层，空白资源的原入口会访问空容器",
                ));
            }
            let rebuild = matches!(
                transform,
                ResourceTransform::Rotate(_)
                    | ResourceTransform::Flip(_)
                    | ResourceTransform::Resize(_)
            );
            for (index, &source) in resource.sources.iter().enumerate() {
                if let ResourceTransform::Blur(radius) = transform {
                    let image = images.get_mut(resource.image)?;
                    image.expand_for_blur(radius)?;
                    resource.geometry.size = image.info.size;
                    resource.geometry.offset = image.info.offset;
                }
                let image = images.get_mut(source)?;
                match transform {
                    ResourceTransform::Color(argb) => image.multiply_color(argb),
                    ResourceTransform::Rgb(rgb) => image.adjust_rgb_percent(rgb),
                    ResourceTransform::Alpha(percent) => image.adjust_alpha_percent(percent),
                    ResourceTransform::Filter(filter) => filter.apply(image),
                    ResourceTransform::Rotate(angle) => {
                        image.rotate_right_angle(angle)?;
                    }
                    ResourceTransform::Flip(horizontal) => image.flip(horizontal),
                    ResourceTransform::Resize(size) => {
                        image.resize_canvas(size)?;
                    }
                    ResourceTransform::Blur(radius) => {
                        image.blur(radius)?;
                    }
                }
                if !rebuild {
                    if let Some(list) = &mut resource.composition {
                        list.invalidate_source(index as u32, images)?;
                    }
                }
            }
            if rebuild {
                resource.rebuild(images, tiles)?;
            }
            resource.updated = true;
            Ok(())
        })
    }

    pub(crate) fn refresh_resource(&mut self, handle: u32) -> Result<()> {
        if self.resource_entry(handle)?.composition.is_none() {
            return Ok(());
        }
        self.with_resource(handle, |resource, images| {
            if let Some(list) = &mut resource.composition {
                list.update(images)?;
            }
            Ok(())
        })
    }

    fn with_resource(
        &mut self,
        handle: u32,
        update: impl FnOnce(&mut ImageResource, &mut Self) -> Result<()>,
    ) -> Result<()> {
        self.resource_entry(handle)?;
        let mut resource = self.resources.remove(&handle).unwrap();
        let result = update(&mut resource, self);
        self.resources.insert(handle, resource);
        result
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
            resource.release(self)?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ImageList, ImageListProperties, Rect};
    use crate::hg3::{DecodedFrame, StandardInfo};

    #[test]
    fn resource_sources_outlive_owner_and_preserve_clean_tiles_and_alpha_classification() {
        let mut images = Images::default();
        let mut layers = Vec::new();
        for (x, color) in [(0, 0x403c_2814u32), (4, 0x80a0_b4c8)] {
            let mut image = Image::new([1, 1]).unwrap();
            image.info.offset = [x, 0];
            image.info.canvas_size = [5, 1];
            image.pixels_mut().copy_from_slice(&color.to_le_bytes());
            layers.push((image, Some(true)));
        }
        let handle = images
            .insert_layered_resource(Some(42), layers, Some([1, 1]))
            .unwrap();
        let sources = images.resource_entry(handle).unwrap().sources.clone();
        let output = images.resource_entry(handle).unwrap().image;
        images.retain_resource(handle).unwrap();
        images.release_resources(42).unwrap();
        images.release_owner(42);
        images
            .get_mut(output)
            .unwrap()
            .pixels_mut()
            .copy_from_slice(&0xff12_3456u32.to_le_bytes().repeat(5));
        images
            .transform_resource(handle, ResourceTransform::Alpha(50), None)
            .unwrap();
        assert_eq!(
            images.get(sources[0]).unwrap().surface().pixels(),
            [20, 40, 60, 32]
        );
        assert_eq!(
            images.get(sources[1]).unwrap().surface().pixels(),
            [200, 180, 160, 64]
        );
        assert_eq!(
            images.resource(handle).unwrap().surface().pixels(),
            0xff12_3456u32.to_le_bytes().repeat(5)
        );
        images.refresh_resource(handle).unwrap();
        let mut expected = 0xff12_3456u32.to_le_bytes().repeat(5);
        expected[..4].copy_from_slice(&[20, 40, 60, 32]);
        expected[16..].copy_from_slice(&[200, 180, 160, 64]);
        assert_eq!(
            images.resource(handle).unwrap().surface().pixels(),
            expected
        );
        images
            .transform_resource(handle, ResourceTransform::Color(0x80ff_8040), None)
            .unwrap();
        assert_eq!(
            images.get(sources[0]).unwrap().surface().pixels(),
            [5, 20, 60, 16]
        );
        assert_eq!(
            images.get(sources[1]).unwrap().surface().pixels(),
            [50, 90, 160, 32]
        );
        assert_eq!(
            images.resource(handle).unwrap().surface().pixels(),
            expected
        );
        images.refresh_resource(handle).unwrap();
        expected[..4].copy_from_slice(&[5, 20, 60, 16]);
        expected[16..].copy_from_slice(&[50, 90, 160, 32]);
        assert_eq!(
            images.resource(handle).unwrap().surface().pixels(),
            expected
        );
        for &source in &sources {
            assert!(images.uses_alpha(source).unwrap());
        }
        images.release_resource_reference(handle).unwrap();
        assert!(images.resources.is_empty());
        assert!(images.entries.is_empty());
        assert!(images.data.is_empty());
    }

    #[test]
    fn rgb_masks_clamp_raw_channels_and_keep_clipped_rgb_and_opacity() {
        let mut images = Images::default();
        let source = images
            .insert_resource(
                None,
                Image::from_frame(DecodedFrame {
                    id: 0,
                    info: StandardInfo {
                        size: [3, 2],
                        bit_depth: 24,
                        canvas_size: [3, 2],
                        offset: [0; 2],
                        base: [0; 2],
                        reserved: 0,
                    },
                    pixels: vec![
                        1, 2, 3, 255, 4, 5, 6, 255, 127, 128, 129, 255, 7, 8, 9, 255, 10, 11, 12,
                        255, 254, 255, 0, 255,
                    ],
                    attributes: Default::default(),
                })
                .unwrap(),
                true,
                Some(false),
            )
            .unwrap();
        let background = images.create(99, [3, 3]).unwrap();
        images
            .get_mut(background)
            .unwrap()
            .pixels_mut()
            .copy_from_slice(&0x8012_3456u32.to_le_bytes().repeat(9));
        let mut list = ImageList::new(99, [3, 3], &mut images).unwrap();
        list.set(0, background, ImageListProperties::default(), &mut images)
            .unwrap();
        for opacity in [1, 255] {
            list.set_resource(
                1,
                source,
                ImageListProperties {
                    position: [-1., 0.],
                    opacity,
                    ..Default::default()
                },
                &mut images,
            )
            .unwrap();
            list.set_clip(1, Some(Rect::from_xywh(1, 0, 2, 3)), &images)
                .unwrap();
            for (channel, alphas) in [
                (1, [64, 128]),
                (2, [65, 128]),
                (3, [65, 0]),
                (4, [65, 0]),
                (5, [65, 0]),
                (0x7fff_ffff, [65, 0]),
                (0x8000_0000, [65, 0]),
                (0x8000_0001, [64, 128]),
                (u32::MAX, [64, 128]),
            ] {
                let wrapper = images.resource_entry(source).unwrap().image;
                images
                    .get_mut(wrapper)
                    .unwrap()
                    .set_alpha_mask(channel)
                    .unwrap();
                list.redraw(&mut images).unwrap();
                let mut expected = 0x8012_3456u32.to_le_bytes().repeat(9);
                expected[4 * 4 + 3] = alphas[0];
                expected[7 * 4 + 3] = alphas[1];
                assert_eq!(
                    images.get(list.image()).unwrap().surface().pixels(),
                    expected
                );
            }
        }
    }

    #[test]
    fn rebuilt_resource_discards_empty_bounds_before_position_can_wrap_them_onscreen() {
        let mut images = Images::default();
        let mut image = Image::new([2, 1]).unwrap();
        image.info_mut().offset = [5, 9];
        image.info_mut().canvas_size = [2, 1];
        image.info_mut().base = [7, 8];
        image.resize_canvas([0, 2]).unwrap();
        let handle = images
            .insert_resource(None, image, true, Some(false))
            .unwrap();
        assert_eq!(images.resource_entry(handle).unwrap().geometry.size, [0; 2]);
        assert_eq!(
            images.resource_entry(handle).unwrap().geometry.offset,
            [0; 2]
        );
        images
            .transform_resource(handle, ResourceTransform::Rotate(0), None)
            .unwrap();
        let geometry = images.resource_entry(handle).unwrap().geometry;
        assert_eq!(geometry.size, [0; 2]);
        assert_eq!(geometry.offset, [0; 2]);
        assert_eq!(geometry.canvas_size, [0; 2]);
        assert_eq!(geometry.base, [0, 16]);

        let mut image = Image::new([2, 1]).unwrap();
        image.info_mut().offset = [i32::MAX, 0];
        let handle = images
            .insert_resource(None, image, true, Some(false))
            .unwrap();
        images
            .transform_resource(handle, ResourceTransform::Rotate(0), None)
            .unwrap();
        let mut list = ImageList::new(99, [4, 2], &mut images).unwrap();
        list.update(&mut images).unwrap();
        list.set_resource(
            0,
            handle,
            ImageListProperties {
                position: [i32::MIN as f32, 0.],
                ..Default::default()
            },
            &mut images,
        )
        .unwrap();
        assert!(list.dirty_rectangles().next().is_none());
        list.release(&mut images).unwrap();
    }
}
