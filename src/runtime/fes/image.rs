use super::{ObjectState, Objects};
use crate::graphics::Rect;
use crate::runtime::{ObjectFactory, Runtime, kind};
use crate::{Error, Result};

impl<F: ObjectFactory> Runtime<F> {
    pub(super) fn update_fes_cimage(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match objects.get(handle).unwrap().image {
            Some(image) => image,
            None => {
                let image = self.images_mut().create_empty(owner)?;
                objects.get_mut(handle).unwrap().image = Some(image);
                image
            }
        };
        let state = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
        let prepared = state.updates.rebuild;
        if prepared {
            if let Some(resource) = &state.size_resource {
                state.size = self.fes_image_geometry(resource, true)?;
            }
            if let Err(error) = self
                .images_mut()
                .recreate(wrapper, state.size.map(|v| v as u32))
            {
                state.updates.rebuild = false;
                return Err(error);
            }
            let image = self.images_mut().get_mut(wrapper)?;
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
            let pixels = self.images().pixel_handle(wrapper)?;
            let state = objects.get(handle).unwrap().state.image().unwrap();
            let slot = state.parent_priority as u32;
            if pixels == 0 {
                self.fes_command(owner, Some(list), 287, [slot, 0, 0], None)?;
            } else if slot != u32::MAX {
                let position = objects.image_position(handle);
                let mut record = [
                    pixels,
                    0,
                    0,
                    0,
                    (position[0] + plane_offset[0] as f32).to_bits(),
                    (position[1] + plane_offset[1] as f32).to_bits(),
                    state.base_point[0].to_bits(),
                    state.base_point[1].to_bits(),
                    state.rotation.to_bits(),
                    state.scale[0].to_bits(),
                    state.scale[1].to_bits(),
                    state.blend as u32,
                    state.display as u32,
                    0,
                    0,
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
    fn image_position(&self, handle: u32) -> [f32; 2] {
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
