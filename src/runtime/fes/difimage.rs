use super::{ObjectState, Objects};
use crate::graphics::{Image, Rect, Surface};
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result};

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
        let size = self.host.surface_size;
        if !self
            .resources
            .render_offscreen(size, range.map(|value| value as f32))?
        {
            return Ok(None);
        }
        let frame = self.resources.offscreen_frame.as_ref().unwrap();
        let rectangle = self
            .resources
            .capture_rectangle
            .unwrap_or_else(|| Rect::from_xywh(0, 0, size[0] as i32, size[1] as i32));
        let extent = [
            rectangle.right.wrapping_sub(rectangle.left),
            rectangle.bottom.wrapping_sub(rectangle.top),
        ];
        if extent.iter().any(|&value| value < 0) {
            return Err(Error::new(0, "画面捕获区域尺寸不能为负数"));
        }
        let mut snapshot = Surface::new(extent.map(|value| value as u32))?;
        snapshot.copy_from(frame.tiles()[0].as_ref().unwrap(), rectangle, [0; 2]);
        let mut image = Image::new(snapshot.size())?;
        image.pixels_mut().copy_from_slice(snapshot.pixels());
        self.resources
            .images
            .insert_resource(None, image, false)
            .map(Some)
    }
}
