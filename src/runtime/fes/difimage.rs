use super::{ObjectState, Objects};
use crate::Result;
use crate::runtime::{ObjectFactory, Store};

/// DIFIMAGE 的基础图像属性与一次性捕获请求。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DifImageState {
    pub base: crate::fes::CImageState,
    pub(super) capture: Option<[u32; 2]>,
    pub(super) resource: Option<u32>,
}

impl DifImageState {
    /// 当前持有的独立图像资源；与普通图像包装句柄分别使用。
    pub fn resource_handle(&self) -> Option<u32> {
        self.resource
    }
    /// 原始无符号深度范围；多次请求覆盖前一组范围。
    pub fn pending_capture(&self) -> Option<[u32; 2]> {
        self.capture
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_difimage(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let list = self.check_fes_image_list(owner, objects)?;
        let ObjectState::DifImage(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        if let Some(range) = state.capture.take() {
            if let Some(source) = self.capture_fes_image(range)? {
                let object = objects.get_mut(handle).unwrap();
                let ObjectState::DifImage(state) = &mut object.state else {
                    unreachable!()
                };
                let previous = state.resource.replace(source);
                object.image = Some(self.resources.images.resource_entry(source)?.image);
                object.image_offset = [0.; 2];
                state.base.base_point = [0.; 2];
                if let Some(previous) = previous {
                    self.resources.images.release_resource_reference(previous)?;
                }
                objects.invalidate(handle)?;
            }
        }
        let object = objects.get_mut(handle).unwrap();
        if let Some(filter) = object.state.image_mut().unwrap().take_filter() {
            let ObjectState::DifImage(state) = &object.state else {
                unreachable!()
            };
            if let Some(resource) = state.resource {
                let source = self.resources.images.resource_entry(resource)?.image;
                filter.apply(self.resources.images.get_mut(source)?);
                if filter != crate::graphics::ImageFilter::None {
                    self.resources.images.resource_entry_mut(resource)?.updated = true;
                }
            }
        }
        let source = objects.get(handle).unwrap().image.unwrap_or(0);
        self.register_fes_image(owner, objects, handle, list, source, false)?;
        self.update_fes_image_priority(owner, objects, handle, list)
    }

    fn capture_fes_image(&mut self, range: [u32; 2]) -> Result<Option<u32>> {
        let Some(image) = self
            .resources
            .capture_image(self.host.surface_size, range.map(|value| value as f32))?
        else {
            return Ok(None);
        };
        self.resources
            .images
            .insert_resource(None, image, false)
            .map(Some)
    }
}
