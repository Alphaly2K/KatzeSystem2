use super::{ObjectFactory, Runtime};
use crate::graphics::{DrawList, Image, Rect, RenderTarget};
use crate::kcs::HostState;
use crate::{Error, Result};

/// A request to record the frame being submitted as a BMP. Submit readback once
/// after presenting the frame, or drop the request if presentation is skipped.
pub struct SequenceCapture<'a> {
    host: &'a mut HostState,
    rectangle: Option<Rect>,
}

impl<F: ObjectFactory> Runtime<F> {
    /// Borrow the draw list and canvas for synchronous presentation. The canvas
    /// dirty flags may be consumed after uploading its textures. When capture is
    /// requested, return the presented pixels through the supplied capture token.
    pub fn present_frame<T>(
        &mut self,
        present: impl FnOnce(&DrawList, &mut RenderTarget, Option<SequenceCapture<'_>>) -> Result<T>,
    ) -> Result<T> {
        let frame = self.compose_frame()?;
        let capture = self.wants_sequence_capture().then_some(SequenceCapture {
            host: &mut self.host,
            rectangle: self.resources.capture_rectangle,
        });
        let canvas = self
            .resources
            .compositor
            .drawing
            .target
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        present(&frame, canvas, capture)
    }

    /// Access the draw list and canvas without submitting a presentation capture.
    pub fn with_frame<T>(
        &mut self,
        render: impl FnOnce(&DrawList, &mut RenderTarget) -> Result<T>,
    ) -> Result<T> {
        self.present_frame(|frame, canvas, _| render(frame, canvas))
    }

    /// Check whether the host should read back this frame for the BMP sequence.
    pub fn wants_sequence_capture(&self) -> bool {
        let drawing = &self.resources.compositor.drawing;
        self.host.frame_capture_enabled != 0 && drawing.mode == 0 && drawing.enabled
    }

    /// 为当前呈现帧执行连续 BMP 输出；未启用或绘图设备关闭时返回 false。
    /// 宿主在每次提交帧后调用一次。
    pub fn capture_sequence_frame(&mut self, frame: &RenderTarget) -> Result<bool> {
        self.sequence_capture()
            .map_or(Ok(false), |capture| capture.write_render_target(frame))
    }

    /// Submit host readback for callers that manage presentation separately.
    /// See [`SequenceCapture::write_bgra`] for the pixel format.
    pub fn capture_sequence_bgra(
        &mut self,
        size: [u32; 2],
        row_stride: usize,
        pixels: &[u8],
    ) -> Result<bool> {
        self.sequence_capture().map_or(Ok(false), |capture| {
            capture.write_bgra(size, row_stride, pixels)
        })
    }

    fn sequence_capture(&mut self) -> Option<SequenceCapture<'_>> {
        self.wants_sequence_capture().then_some(SequenceCapture {
            host: &mut self.host,
            rectangle: self.resources.capture_rectangle,
        })
    }
}

impl SequenceCapture<'_> {
    /// Submit the software-rendered frame.
    pub fn write_render_target(self, frame: &RenderTarget) -> Result<bool> {
        self.write_frame(frame.size(), |x, y| frame.pixel(x, y))
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

    fn write_frame(self, size: [u32; 2], pixel: impl Fn(u32, u32) -> Option<u32>) -> Result<bool> {
        let Some(bytes) = Image::encode_frame_bmp(size, self.rectangle, pixel)? else {
            return Ok(false);
        };
        let index = self.host.frame_capture_index;
        self.host.frame_capture_index = index.wrapping_add(1);
        let folder = &self.host.frame_capture_folder;
        let mut name = Vec::with_capacity(folder.len() * 2 + 20);
        name.extend_from_slice(folder);
        name.push(92);
        name.extend_from_slice(folder);
        name.extend(format!("_{:010}.bmp", index as i32).encode_utf16());
        if let Some(mut writer) = self
            .host
            .file_services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置连续截图文件输出服务"))?
            .create_utf16(self.host.frame_capture_task_id, &name)?
        {
            let _ = writer.write(&bytes);
        }
        Ok(true)
    }
}
