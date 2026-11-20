//! Frame presentation data and synchronous rendering for script readback.

mod capture;
mod commands;
pub use capture::SequenceCapture;
pub use commands::{
    DrawCommand, DrawList, FrameTexture, FrameTriangle, TextureAddress, TextureFilter,
    TextureSource,
};

pub use crate::graphics::{ProjectedVertex, Rect, Surface};
use crate::{Result, graphics::RenderTarget};

/// Read-only canvas textures with an explicit upload acknowledgement. A host
/// cache shared by presentation and script readback can consume the dirty flags.
pub struct FrameCanvas<'a>(&'a mut RenderTarget);

impl<'a> FrameCanvas<'a> {
    /// Wrap a canvas for standalone rendering outside a game session.
    pub fn new(target: &'a mut RenderTarget) -> Self {
        Self(target)
    }

    /// Logical canvas size in pixels, including unallocated tiles.
    pub fn size(&self) -> [u32; 2] {
        self.0.size()
    }

    /// Texture slots in canvas order. `None` denotes an unallocated tile.
    pub fn tiles(&self) -> &[Option<Surface>] {
        self.0.tiles()
    }

    /// Upload flags indexed in the same order as `tiles`.
    pub fn dirty_tiles(&self) -> &[bool] {
        self.0.dirty_tiles()
    }

    /// Resolve a draw command's texture, including standalone images.
    pub fn texture<'b>(&'b self, source: &'b TextureSource) -> Option<&'b Surface> {
        source.surface(self.0)
    }

    /// Clear dirty flags after uploading all changed tiles to the shared cache.
    pub fn acknowledge_upload(&mut self) {
        self.0.clear_dirty();
    }

    pub(crate) fn target(&self) -> &RenderTarget {
        self.0
    }
}

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
