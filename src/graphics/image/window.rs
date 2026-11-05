use super::{Image, ImageData, Images, Rect};
use crate::{Error, Result, hg3};

impl Image {
    /// 将 HG-3 的 0～8 帧拼接为指定尺寸的窗口图像，缺失帧跳过。
    pub fn from_window(bytes: &[u8], size: [u32; 2]) -> Result<Self> {
        let mut image = Self::new(size)?;
        hg3::Metadata::parse(bytes)?;
        for id in 0..9 {
            let Some(frame) = hg3::decode_frame(bytes, id)? else {
                continue;
            };
            let mut part = Self::from_frame(frame)?;
            part.use_pixel_canvas();
            let [width, height] = part.info().size;
            let [w, h] = size;
            let columns = matches!(id, 6..=8);
            let rows = matches!(id, 4 | 5 | 8);
            // 原平铺循环在减法回绕时不能正常结束。
            if (columns && w < width) || (rows && h < height) {
                return Err(Error::new(0, "窗口尺寸小于平铺块，原循环发生无符号回绕"));
            }
            let x_range = if columns { width..w - width } else { 0..1 };
            let y_range = if rows { height..h - height } else { 0..1 };
            for y in y_range.step_by(if rows { height as usize } else { 1 }) {
                for x in x_range
                    .clone()
                    .step_by(if columns { width as usize } else { 1 })
                {
                    let x = if matches!(id, 1 | 3 | 5) {
                        (w as i32).wrapping_sub(width as i32)
                    } else {
                        x as i32
                    };
                    let y = if matches!(id, 2 | 3 | 7) {
                        (h as i32).wrapping_sub(height as i32)
                    } else {
                        y as i32
                    };
                    let copy_width = if columns {
                        width.min(w - x as u32 - width)
                    } else {
                        width
                    };
                    let copy_height = if rows {
                        height.min(h - y as u32 - height)
                    } else {
                        height
                    };
                    image.blend_from(
                        &part,
                        Rect::from_xywh(0, 0, copy_width as i32, copy_height as i32),
                        [x, y],
                    );
                }
            }
        }
        Ok(image)
    }
}

impl Images {
    pub(crate) fn build_window(&mut self, handle: u32, bytes: &[u8], size: [u32; 2]) -> Result<()> {
        self.bind_pixels(handle, 0)?;
        let image = Image::from_window(bytes, size)?;
        let pixels = self.handles.allocate()?;
        self.data.insert(
            pixels,
            ImageData {
                references: 1,
                has_alpha: None,
                image,
            },
        );
        self.entries.get_mut(&handle).unwrap().data = pixels;
        Ok(())
    }
}
