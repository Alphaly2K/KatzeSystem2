use super::Rect;
use crate::{Error, Result, graphics::Image};

/// A request to record the frame being submitted as a BMP. Submit readback once
/// after presenting the frame, or drop the request if presentation is skipped.
pub struct SequenceCapture<'a> {
    rectangle: Option<Rect>,
    write_bmp: &'a mut dyn FnMut(&[u8]) -> Result<()>,
}

impl<'a> SequenceCapture<'a> {
    pub(crate) fn new(
        rectangle: Option<Rect>,
        write_bmp: &'a mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Self {
        Self {
            rectangle,
            write_bmp,
        }
    }

    /// Submit top-down BGRA pixels, with `row_stride` bytes between row starts.
    /// Alpha and padding are ignored. Invalid input does not advance the index;
    /// an empty frame or an empty capture region returns false.
    pub fn write_bgra(self, size: [u32; 2], row_stride: usize, pixels: &[u8]) -> Result<bool> {
        if size.contains(&0) {
            return Ok(false);
        }
        if size.iter().any(|&axis| axis > i32::MAX as u32) {
            return Err(Error::new(0, "截图像素尺寸超出坐标范围"));
        }
        let row_bytes = (size[0] as usize)
            .checked_mul(4)
            .filter(|&bytes| bytes <= row_stride)
            .ok_or_else(|| Error::new(0, "截图像素行距不足"))?;
        (size[1] as usize - 1)
            .checked_mul(row_stride)
            .and_then(|offset| offset.checked_add(row_bytes))
            .filter(|&length| length <= pixels.len())
            .ok_or_else(|| Error::new(0, "截图像素缓冲区长度不足"))?;
        self.write_frame(size, |x, y| {
            let at = y as usize * row_stride + x as usize * 4;
            Some(u32::from_le_bytes(pixels[at..at + 4].try_into().unwrap()))
        })
    }

    pub(crate) fn write_frame(
        self,
        size: [u32; 2],
        pixel: impl Fn(u32, u32) -> Option<u32>,
    ) -> Result<bool> {
        let Some(bytes) = Image::encode_frame_bmp(size, self.rectangle, pixel)? else {
            return Ok(false);
        };
        (self.write_bmp)(&bytes)?;
        Ok(true)
    }
}
