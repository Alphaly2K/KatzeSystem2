use super::{ObjectFactory, Runtime};
use crate::graphics::RenderTarget;
use crate::host::graphics::{DrawList, FrameCanvas, SequenceCapture};
use crate::{Error, Result};

impl<F: ObjectFactory> Runtime<F> {
    /// Borrow the draw list and canvas for synchronous presentation. The canvas
    /// dirty flags may be consumed after uploading its textures. When capture is
    /// requested, return the presented pixels through the supplied capture token.
    pub fn present_frame<T>(
        &mut self,
        present: impl FnOnce(&DrawList, &mut FrameCanvas<'_>, Option<SequenceCapture<'_>>) -> Result<T>,
    ) -> Result<T> {
        let frame = self.compose_frame()?;
        let capture = self
            .wants_sequence_capture()
            .then(|| SequenceCapture::new(&mut self.host, self.resources.capture_rectangle));
        let canvas = self
            .resources
            .compositor
            .drawing
            .target
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        present(&frame, &mut FrameCanvas::new(canvas), capture)
    }

    /// Access the draw list and canvas without submitting a presentation capture.
    pub fn with_frame<T>(
        &mut self,
        render: impl FnOnce(&DrawList, &mut FrameCanvas<'_>) -> Result<T>,
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
        self.wants_sequence_capture()
            .then(|| SequenceCapture::new(&mut self.host, self.resources.capture_rectangle))
    }
}
