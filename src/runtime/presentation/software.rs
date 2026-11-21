use super::{ObjectFactory, Runtime};
use crate::{
    Result,
    graphics::{Projection, RenderTarget},
    host::graphics::{DrawList, TextureFilter},
};

impl<F: ObjectFactory> Runtime<F> {
    /// 以当前画布为纹理输入，向独立目标提交画面；不清空目标或执行设备呈现。
    pub fn render_frame(
        &mut self,
        output: &mut RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        let touched = if let Some(frame) = self.resources.movie_frame(output.size(), filter)? {
            output.draw_list(&frame, self.resources.compositor.drawing.target.as_ref())?
        } else {
            self.resources
                .compositor
                .render_frame(output, projection, parent, filter)?
        };
        if !self.uses_image_cursor() {
            return Ok(touched);
        }
        let mut overlay = DrawList {
            commands: Vec::new(),
            filter,
        };
        self.resources.append_image_cursor(&mut overlay)?;
        Ok(output.draw_list(&overlay, None)? || touched)
    }

    /// 为当前呈现帧执行连续 BMP 输出；未启用或绘图设备关闭时返回 false。
    /// 宿主在每次提交帧后调用一次。
    pub fn capture_sequence_frame(&mut self, frame: &RenderTarget) -> Result<bool> {
        self.submit_sequence_capture(|capture| {
            capture.write_frame(frame.size(), |x, y| frame.pixel(x, y))
        })
    }
}
