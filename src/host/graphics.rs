use crate::{
    Result,
    graphics::{DrawList, FrameCanvas, Surface},
};

/// Host renderer for synchronous, script-initiated readback.
pub trait GraphicsBackend {
    /// Draw onto a transparent target and return top-down BGRA pixels at `size`.
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut FrameCanvas<'_>,
        size: [u32; 2],
    ) -> Result<Surface>;
}
