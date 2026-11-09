use super::{ObjectState, Objects};
use crate::graphics::Rect;
use crate::runtime::{ObjectFactory, Store, kind};
use crate::{Error, Result};

impl Objects {
    pub(super) fn refresh_image_names(
        &mut self,
        languages: &crate::language::LanguageState,
    ) -> Result<()> {
        if &self.languages != languages {
            for object in self.objects.values_mut() {
                if let Some(state) = object.state.image_source_mut() {
                    state.refresh_file(|name| languages.expand_image(name))?;
                }
            }
            self.languages = languages.clone();
        }
        Ok(())
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_image_window(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let cache = objects.preloaded_images.clone();
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let state = objects.get(handle).unwrap().state.image_source().unwrap();
        let mut prepared = false;
        if state.base.updates.needs_rebuild() {
            let name = &state.file[..state
                .file
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(state.file.len())];
            let raw = if name.is_empty() {
                0
            } else {
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置 FES 图像资源服务"))?;
                let mut services = super::preload::ImageServices::new(&cache, services);
                self.resources
                    .raw
                    .load_fes_image(name, &mut services, owner, 0)?
            };
            objects
                .get_mut(handle)
                .unwrap()
                .state
                .image_mut()
                .unwrap()
                .updates
                .source_rebuild = false;
            if raw != 0 {
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .image_mut()
                    .unwrap()
                    .updates
                    .rebuild = false;
                let result = (|| {
                    let reference = objects
                        .get(handle)
                        .unwrap()
                        .state
                        .image()
                        .unwrap()
                        .size_resource
                        .clone();
                    if let Some(reference) = reference {
                        let size = self.fes_image_geometry(&cache, &reference, true)?;
                        objects
                            .get_mut(handle)
                            .unwrap()
                            .state
                            .image_mut()
                            .unwrap()
                            .set_size(size);
                    }
                    let size = objects.get(handle).unwrap().state.image().unwrap().size;
                    if size.into_iter().all(|axis| axis > 0) {
                        self.resources.images.build_window(
                            wrapper,
                            self.resources.raw.image_data(raw)?,
                            size.map(|v| v as u32),
                        )?;
                    }
                    Ok(())
                })();
                self.resources.raw.release_image(raw)?;
                result?;
                prepared = true;
            }
        }
        self.register_fes_image(owner, objects, handle, list, wrapper, prepared)?;
        self.update_fes_image_priority(owner, objects, handle, list)
    }

    pub(super) fn update_fes_cimage(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let cache = objects.preloaded_images.clone();
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
        let prepared = state.updates.rebuild;
        if prepared {
            if let Some(resource) = &state.size_resource {
                state.set_size(self.fes_image_geometry(&cache, resource, true)?);
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
        let object = objects.get(handle).unwrap();
        let displayed = object.reference_image.or(object.image).unwrap();
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
        let cache = objects.preloaded_images.clone();
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let state = objects.get(handle).unwrap().state.image_source().unwrap();
        let prepared = state.base.updates.needs_rebuild();
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
                    let mut services = super::preload::ImageServices::new(&cache, services);
                    self.resources
                        .raw
                        .load_fes_image(name, &mut services, owner, 0)?
                };
                let loaded_name = String::from_utf8_lossy(name).into_owned();
                let object_name = objects.get(handle).unwrap().name.clone();
                let object = objects.get_mut(handle).unwrap();
                self.resources.raw.release_image(object.raw_image)?;
                object.raw_image = raw;
                if std::env::var_os("KATZE_TRACE_NATIVE").is_some() {
                    eprintln!(
                        "[img-raw] object={object_name} handle={handle} name={loaded_name:?} raw={raw}"
                    );
                }
                if raw != 0
                    && matches!(object.state, ObjectState::Button(_))
                    && object.state.image().unwrap().size.contains(&-1)
                {
                    let size = {
                        let bytes = self.resources.raw.image_data(raw)?;
                        crate::hg3::Metadata::parse(bytes)
                            .ok()
                            .and_then(|metadata| {
                                metadata
                                    .frames
                                    .first()
                                    .and_then(|frame| frame.info)
                                    .map(|info| info.size)
                            })
                    };
                    if let Some(size) = size {
                        objects
                            .get_mut(handle)
                            .unwrap()
                            .state
                            .image_mut()
                            .unwrap()
                            .set_size(size.map(|v| v as i32));
                    }
                }
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
                if direct {
                    self.resources.images.load_fes(
                        wrapper,
                        name,
                        state.selected_frame(),
                        true,
                        services.as_mut(),
                    )?
                } else {
                    let mut services =
                        super::preload::ImageServices::new(&cache, services.as_mut());
                    self.resources.images.load_fes(
                        wrapper,
                        name,
                        state.selected_frame(),
                        false,
                        &mut services,
                    )?
                }
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
                        state.set_size(info.size.map(|v| v as i32));
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
                let object = objects.get_mut(handle).unwrap();
                if let Some(reference) = object.reference_image {
                    self.resources.images.release_reference(reference)?;
                    object.reference_image = None;
                }
                object.image = Some(image);
                object.images.push(image);
                Ok(image)
            }
        }
    }

    pub(super) fn register_fes_image(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        list: u32,
        wrapper: u32,
        prepared: bool,
    ) -> Result<()> {
        let cache = objects.preloaded_images.clone();
        let object = objects.get(handle).unwrap();
        let file = object.state.image_source().is_some();
        let window = matches!(object.state, ObjectState::ImageWindow(_));
        let captured = matches!(objects.get(handle).unwrap().state, ObjectState::DifImage(_));
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
                    .fes_image_geometry(&cache, &reference, false)?
                    .map(|v| v as f32);
                if state.position != position {
                    state.position = position;
                    objects.invalidate(handle)?;
                }
            }
            let wrapper = objects
                .get(handle)
                .unwrap()
                .reference_image
                .unwrap_or(wrapper);
            let pixels = if wrapper == 0 || captured {
                0
            } else {
                self.resources.images.pixel_handle(wrapper)?
            };
            if wrapper != 0 && self.resources.images.pixel_handle(wrapper)? != 0 {
                let mask = objects.get(handle).unwrap().state.image().unwrap().mask as u32;
                self.resources
                    .images
                    .get_mut(wrapper)?
                    .set_alpha_mask(mask)?;
            }
            let raw = objects.get(handle).unwrap().raw_image;
            let state = objects.get(handle).unwrap().state.image().unwrap();
            let slot = state.parent_priority as u32;
            if pixels == 0 && raw == 0 && (!captured || wrapper == 0) {
                if !file || slot != u32::MAX {
                    self.fes_command(owner, Some(list), 287, [slot, 0, 0], None)?;
                }
            } else if (file && !window) || slot != u32::MAX {
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
                    if let ObjectState::DifImage(state) = &objects.get(handle).unwrap().state {
                        state.resource.unwrap_or(0)
                    } else {
                        0
                    },
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
                    if pixels == 0 && !captured { wrapper } else { 0 },
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
            updates.source_rebuild = false;
            updates.dirty = false;
        }
        Ok(())
    }

    pub(super) fn update_fes_image_priority(
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
            .find_reachable(owner, list)
            .is_some_and(|node| node.kind == kind::IMAGE_LIST)
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
            next = object.parent.filter(|&parent| {
                matches!(
                    self.get(parent).unwrap().state,
                    ObjectState::CImage(_) | ObjectState::Image(_)
                )
            });
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
