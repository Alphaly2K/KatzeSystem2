use super::{ObjectFactory, Resources, Runtime};
use crate::{
    Error, Result,
    graphics::RenderTarget,
    host::graphics::{DrawList, FrameCanvas, GraphicsBackend, SequenceCapture},
    kcs::HostState,
};

impl<F: ObjectFactory> Runtime<F> {
    /// Borrow the draw list and canvas for synchronous presentation. The canvas
    /// dirty flags may be consumed after uploading its textures. When capture is
    /// requested, return the presented pixels through the supplied capture token.
    pub fn present_frame<T>(
        &mut self,
        present: impl FnOnce(&DrawList, &mut FrameCanvas<'_>, Option<SequenceCapture<'_>>) -> Result<T>,
    ) -> Result<T> {
        let frame = self.compose_frame()?;
        let wants_capture = self.wants_sequence_capture();
        let mut write_bmp = |bytes: &[u8]| write_sequence_bmp(&mut self.host, bytes);
        let capture = wants_capture
            .then(|| SequenceCapture::new(self.resources.capture_rectangle, &mut write_bmp));
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
        self.submit_sequence_capture(|capture| {
            capture.write_frame(frame.size(), |x, y| frame.pixel(x, y))
        })
    }

    /// Submit host readback for callers that manage presentation separately.
    /// See [`SequenceCapture::write_bgra`] for the pixel format.
    pub fn capture_sequence_bgra(
        &mut self,
        size: [u32; 2],
        row_stride: usize,
        pixels: &[u8],
    ) -> Result<bool> {
        self.submit_sequence_capture(|capture| capture.write_bgra(size, row_stride, pixels))
    }

    fn submit_sequence_capture(
        &mut self,
        submit: impl FnOnce(SequenceCapture<'_>) -> Result<bool>,
    ) -> Result<bool> {
        if !self.wants_sequence_capture() {
            return Ok(false);
        }
        let mut write_bmp = |bytes: &[u8]| write_sequence_bmp(&mut self.host, bytes);
        submit(SequenceCapture::new(
            self.resources.capture_rectangle,
            &mut write_bmp,
        ))
    }
}

fn write_sequence_bmp(host: &mut HostState, bytes: &[u8]) -> Result<()> {
    let index = host.frame_capture_index;
    host.frame_capture_index = index.wrapping_add(1);
    let folder = &host.frame_capture_folder;
    let mut name = Vec::with_capacity(folder.len() * 2 + 20);
    name.extend_from_slice(folder);
    name.push(92);
    name.extend_from_slice(folder);
    name.extend(format!("_{:010}.bmp", index as i32).encode_utf16());
    if let Some(mut writer) = host
        .file_services
        .as_deref_mut()
        .ok_or_else(|| Error::new(0, "运行时未配置连续截图文件输出服务"))?
        .create_utf16(host.frame_capture_task_id, &name)?
    {
        let _ = writer.write(bytes);
    }
    Ok(())
}

impl Resources {
    pub fn compose_frame(&mut self, range: Option<[f32; 2]>) -> Result<DrawList> {
        self.update_plane_movie_frames()?;
        if range.is_none() {
            let size = self.movie.as_ref().map(|movie| movie.surface_size);
            if let Some(size) = size {
                let filter = self
                    .compositor
                    .frame_view
                    .as_ref()
                    .map_or(crate::graphics::TextureFilter::Linear, |view| view.2);
                if let Some(frame) = self.movie_frame(size, filter)? {
                    return Ok(frame);
                }
            }
        }
        let backend = self
            .readback_backend
            .as_mut()
            .map(|backend| &mut **backend as &mut dyn GraphicsBackend);
        self.compositor.compose_with_backend(range, backend)
    }
}
