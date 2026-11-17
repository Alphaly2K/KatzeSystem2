use std::sync::Arc;

use super::{ObjectFactory, Resources, Runtime};
use crate::graphics::{DrawList, Rect, Surface, TextureFilter};
use crate::resource::ResourceMode;
use crate::{Error, Result};

pub(super) struct ImageCursor {
    surface: Arc<Surface>,
    hotspot: [u16; 2],
}

impl<F: ObjectFactory> Runtime<F> {
    /// Whether the game supplies the pointer image when the device points into the canvas.
    pub fn uses_image_cursor(&self) -> bool {
        self.resources.image_cursor.is_some()
            && self.resources.input_device.is_some()
            && self.resources.image_cursor_drawing_enabled()
            && self
                .host
                .platform_settings
                .as_ref()
                .is_none_or(|settings| settings.window_cursor_mode == 1)
    }
}

impl Resources {
    fn image_cursor_drawing_enabled(&self) -> bool {
        let drawing = &self.compositor.drawing;
        drawing.enabled && drawing.mode == 0 && drawing.target.is_some()
    }

    pub(super) fn load_image_cursor(&mut self, name: &[u8], hotspot: [u16; 2]) -> Result<()> {
        if !self.image_cursor_drawing_enabled() {
            return Ok(());
        }
        self.image_cursor = None;
        if name.is_empty() {
            return Ok(());
        }
        let wide: Vec<_> = crate::text::cp932_replacing(name).encode_utf16().collect();
        if wide.len() >= 32 {
            return Err(Error::new(0, "光标资源名超过基线缓冲区范围"));
        }
        let services = self
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置资源服务"))?;
        let Some(bytes) = services.read_utf16(&wide, ResourceMode::LooseFirst)? else {
            return Ok(());
        };
        let frame = match bytes.get(..4) {
            Some(b"HG-3") => crate::hg3::decode_frame(&bytes, 0)?,
            Some(b"HG-2") => Some(crate::hg2::decode_frame(&bytes)?),
            _ => return Err(Error::new(0, "不支持的光标图像格式")),
        };
        if let Some(frame) = frame {
            let surface = Surface::from_bgra(frame.info.size, frame.pixels)?;
            self.image_cursor = Some(ImageCursor {
                surface: Arc::new(surface),
                hotspot,
            });
        }
        Ok(())
    }

    pub(super) fn append_image_cursor(&mut self, frame: &mut DrawList) -> Result<()> {
        let (Some(cursor), Some(device)) = (&self.image_cursor, self.input_device.as_deref_mut())
        else {
            return Ok(());
        };
        if device.cursor_visible()? == 0 {
            return Ok(());
        }
        let pointer = device.pointer()?;
        let (point, inside) = self.input_coordinates.to_logical(pointer.position);
        if pointer.active == 0 || !inside {
            return Ok(());
        }
        let [width, height] = cursor.surface.size().map(|n| n as i32);
        let [x, y] = std::array::from_fn(|axis| point[axis] - i32::from(cursor.hotspot[axis]));
        frame.push_screen_quad(
            Rect::from_xywh(x, y, width, height),
            Some(&cursor.surface),
            u32::MAX,
            Some(TextureFilter::Point),
        );
        Ok(())
    }
}
