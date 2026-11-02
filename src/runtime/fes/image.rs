use super::{ObjectState, Objects};
use crate::graphics::Rect;
use crate::runtime::{ObjectFactory, Store, kind};
use crate::{Error, Result};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_cimage(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
        let prepared = state.updates.rebuild;
        if prepared {
            if let Some(resource) = &state.size_resource {
                state.size = self.fes_image_geometry(resource, true)?;
            }
            if let Err(error) = self
                .resources
                .images
                .recreate(wrapper, state.size.map(|v| v as u32))
            {
                state.updates.rebuild = false;
                return Err(error);
            }
            let image = self.resources.images.get_mut(wrapper)?;
            image.use_pixel_canvas();
            image.fill_rect_from_bottom(
                Rect::from_xywh(0, 0, state.size[0], state.size[1]),
                state.color as u32,
            );
            let offset = image.info().offset.map(|v| v as f32);
            let object = objects.get_mut(handle).unwrap();
            if object.image_offset != offset {
                object.image_offset = offset;
                objects.invalidate(handle)?;
            }
        }
        // 重复声明后两个包装可能不同，CIMAGE 的显示跟随对象绑定。
        let displayed = objects.get(handle).unwrap().image.unwrap();
        self.register_fes_image(owner, objects, handle, list, displayed, prepared)?;
        self.update_fes_image_priority(owner, objects, handle, list)
    }

    pub(super) fn update_fes_image(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let state = objects.get(handle).unwrap().state.image_source().unwrap();
        let prepared = state.base.updates.rebuild;
        if prepared {
            let name = &state.file[..state
                .file
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(state.file.len())];
            let direct = state.path_mode != 0;
            let loaded = if state.image_number == -1 {
                let raw = if name.is_empty() {
                    0
                } else {
                    let services = self
                        .resources
                        .services
                        .as_deref_mut()
                        .ok_or_else(|| Error::new(0, "运行时未配置 FES 图像资源服务"))?;
                    self.resources
                        .raw
                        .load_image(name, services, Some(owner), 0)?
                };
                let object = objects.get_mut(handle).unwrap();
                self.resources.raw.release_image(object.raw_image)?;
                object.raw_image = raw;
                false
            } else if name.is_empty() || (!direct && name.iter().all(|&b| b == b' ')) {
                if !direct {
                    self.resources.images.bind_pixels(wrapper, 0)?;
                }
                false
            } else {
                let services = self
                    .resources
                    .services
                    .as_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置 FES 图像资源服务"))?;
                self.resources.images.load_fes(
                    wrapper,
                    name,
                    state.selected_frame(),
                    direct,
                    services.as_mut(),
                )?
            };
            if loaded {
                let image = self.resources.images.get_mut(wrapper)?;
                let info = *image.info();
                let object = objects.get_mut(handle).unwrap();
                let state = object.state.image_mut().unwrap();
                if matches!(state.button_type, 0 | 1) {
                    let (offset, base) = if state.button_type == 0 {
                        image.use_pixel_canvas();
                        (
                            info.base.map(|v| v as f32),
                            std::array::from_fn(|i| {
                                info.base[i].wrapping_sub(info.offset[i]) as f32
                            }),
                        )
                    } else {
                        ([0.; 2], info.base.map(|v| v as f32))
                    };
                    let dirty = object.image_offset != offset || state.base_point != base;
                    object.image_offset = offset;
                    state.base_point = base;
                    if direct || state.size.contains(&-1) {
                        state.size = info.size.map(|v| v as i32);
                    }
                    if dirty {
                        objects.invalidate(handle)?;
                    }
                }
            }
        }
        self.register_fes_image(owner, objects, handle, list, wrapper, prepared)?;
        self.update_fes_image_animation(owner, objects, handle, list)?;
        self.update_fes_image_priority(owner, objects, handle, list)
    }

    pub(super) fn fes_image_wrapper(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<u32> {
        match objects.get(handle).unwrap().image {
            Some(image) => Ok(image),
            None => {
                let image = self.resources.images.create_empty(owner)?;
                objects.get_mut(handle).unwrap().image = Some(image);
                objects.get_mut(handle).unwrap().images.push(image);
                Ok(image)
            }
        }
    }

    fn register_fes_image(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        list: u32,
        wrapper: u32,
        prepared: bool,
    ) -> Result<()> {
        let file = objects.get(handle).unwrap().state.image_source().is_some();
        let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
        if state.updates.dirty || prepared {
            let reference = state.plane.clone();
            let mut plane_offset = [0; 2];
            let mut clip = objects.image_clip;
            if let Some(target) = objects.lookup(&reference.name, reference.bank) {
                if let ObjectState::Plane(plane) = &objects.get(target).unwrap().state {
                    plane_offset = [plane.vram()[0], plane.vram()[1]].map(integer_position);
                    let size = plane.base.size();
                    clip = Rect::from_xywh(plane_offset[0], plane_offset[1], size[0], size[1]);
                }
            }
            let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
            if let Some(reference) = state.position_resource.take() {
                let position = self
                    .fes_image_geometry(&reference, false)?
                    .map(|v| v as f32);
                if state.position != position {
                    state.position = position;
                    objects.invalidate(handle)?;
                }
            }
            let pixels = self.resources.images.pixel_handle(wrapper)?;
            let raw = objects.get(handle).unwrap().raw_image;
            let state = objects.get(handle).unwrap().state.image().unwrap();
            let slot = state.parent_priority as u32;
            if pixels == 0 && raw == 0 {
                if !file || slot != u32::MAX {
                    self.fes_command(owner, Some(list), 287, [slot, 0, 0], None)?;
                }
            } else if file || slot != u32::MAX {
                let position = objects.image_position(handle);
                let position: [f32; 2] = std::array::from_fn(|i| {
                    if file {
                        (plane_offset[i] as f32 + state.base_point[i]) + position[i]
                    } else {
                        position[i] + plane_offset[i] as f32
                    }
                });
                let mut record = [
                    pixels,
                    if pixels == 0 { raw } else { 0 },
                    0,
                    0,
                    position[0].to_bits(),
                    position[1].to_bits(),
                    state.base_point[0].to_bits(),
                    state.base_point[1].to_bits(),
                    state.rotation.to_bits(),
                    state.scale[0].to_bits(),
                    state.scale[1].to_bits(),
                    state.blend as u32,
                    state.display as u32,
                    0,
                    if pixels == 0 { wrapper } else { 0 },
                ]
                .map(u32::to_le_bytes)
                .concat();
                self.fes_command(owner, Some(list), 286, [slot, 0, 0], Some(&mut record))?;
                let mut bytes = [clip.left, clip.top, clip.right, clip.bottom]
                    .map(i32::to_le_bytes)
                    .concat();
                self.fes_command(owner, Some(list), 295, [slot, 0, 0], Some(&mut bytes))?;
            }
            let updates = &mut objects
                .get_mut(handle)
                .unwrap()
                .state
                .image_mut()
                .unwrap()
                .updates;
            updates.rebuild = false;
            updates.dirty = false;
        }
        Ok(())
    }

    fn update_fes_image_priority(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        list: u32,
    ) -> Result<()> {
        let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
        if state.updates.priority_changed {
            state.updates.priority_changed = false;
            self.fes_command(
                owner,
                Some(list),
                305,
                [state.parent_priority as u32, state.priority as u32, 0],
                None,
            )?;
        }
        Ok(())
    }

    pub(super) fn check_fes_image_list(&self, owner: u32, objects: &Objects) -> Result<u32> {
        let list = objects
            .image_list
            .ok_or_else(|| Error::new(0, "FES 尚未绑定共享图像列表"))?;
        if !self
            .tree
            .node(list)
            .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::IMAGE_LIST)
        {
            return Err(Error::new(0, "FES 共享图像列表任务绑定失效"));
        }
        Ok(list)
    }
}

impl Objects {
    pub(super) fn image_position(&self, handle: u32) -> [f32; 2] {
        let mut chain = Vec::new();
        let mut next = Some(handle);
        while let Some(handle) = next {
            let object = self.get(handle).unwrap();
            let Some(image) = object.state.image() else {
                break;
            };
            chain.push((image, object.image_offset));
            next = object.parent;
        }
        chain
            .into_iter()
            .rev()
            .fold([0.; 2], |parent, (image, offset)| {
                std::array::from_fn(|i| {
                    offset[i] + image.position[i] - image.base_point[i] + parent[i]
                })
            })
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
