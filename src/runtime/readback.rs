use super::Resources;
use crate::{
    Error, Result,
    graphics::{Image, RenderTarget},
    host::graphics::{FrameCanvas, Rect, Surface, render_readback},
};

impl Resources {
    pub fn render_offscreen(&mut self, size: [u32; 2], range: [f32; 2]) -> Result<bool> {
        let Some(frame) = self.readback_depth_frame(size, range)? else {
            return Ok(false);
        };
        self.offscreen_frame = Some(RenderTarget::from_tiles(size, [1, 1], vec![Some(frame)])?);
        Ok(true)
    }

    pub(super) fn offscreen_surface(&self) -> Option<&Surface> {
        self.offscreen_frame.as_ref()?.tiles()[0].as_ref()
    }

    pub fn render_auxiliary(
        &mut self,
        index: u32,
        size: [u32; 2],
        range: [f32; 2],
    ) -> Result<bool> {
        if index as usize >= self.compositor.auxiliary_targets.len() {
            return Ok(false);
        }
        let Some(frame) = self.readback_depth_frame(size, range)? else {
            return Ok(false);
        };
        let frame = RenderTarget::from_tiles(size, [1, 1], vec![Some(frame)])?;
        if index == 0 && !self.compositor.shatters.entries.is_empty() {
            let image = frame.tiles()[0].as_ref().cloned().map(std::sync::Arc::new);
            for effect in self.compositor.shatters.entries.values_mut() {
                effect.source = image.clone();
            }
        }
        if index == 0 && !self.compositor.page_flips.is_empty() {
            if let Some(image) = frame.tiles()[0].as_ref() {
                let image = std::sync::Arc::new(image.clone());
                for effect in self.compositor.page_flips.values_mut() {
                    effect.set_source(image.clone());
                }
            }
        }
        if index == 0 && !self.compositor.render_draws.is_empty() {
            if let Some(image) = frame.tiles()[0].as_ref() {
                let image = std::sync::Arc::new(image.clone());
                for effect in self.compositor.render_draws.values_mut() {
                    effect.source = image.clone();
                }
            }
        }
        if !self.compositor.cell_flips.is_empty() {
            if let Some(image) = frame.tiles()[0].as_ref() {
                let image = std::sync::Arc::new(image.clone());
                for effect in self.compositor.cell_flips.values_mut() {
                    effect.set_source(index as usize, image.clone());
                }
            }
        }
        self.compositor.auxiliary_targets[index as usize] = frame;
        Ok(true)
    }

    pub(super) fn readback_depth_frame(
        &mut self,
        size: [u32; 2],
        range: [f32; 2],
    ) -> Result<Option<Surface>> {
        if self.compositor.drawing.mode != 0 || !self.compositor.drawing.enabled {
            return Ok(None);
        }
        if self.compositor.drawing.target.is_none() {
            return Ok(None);
        }
        if self.readback_backend.is_none() {
            return Err(Error::new(0, "宿主未配置脚本离屏绘制后端"));
        }
        let frame = self.compose_frame(Some(range))?;
        let mut canvas = FrameCanvas::new(self.compositor.drawing.target.as_mut().unwrap());
        render_readback(
            self.readback_backend.as_deref_mut().unwrap(),
            &frame,
            &mut canvas,
            size,
        )
        .map(Some)
    }

    pub fn capture_image(&mut self, size: [u32; 2], range: [f32; 2]) -> Result<Option<Image>> {
        if !self.render_offscreen(size, range)? {
            return Ok(None);
        }
        let frame = self.offscreen_surface().unwrap();
        let rectangle = self
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
        snapshot.copy_from(frame, rectangle, [0; 2]);
        let mut image = Image::new(snapshot.size())?;
        image.pixels_mut().copy_from_slice(snapshot.pixels());
        Ok(Some(image))
    }
}
