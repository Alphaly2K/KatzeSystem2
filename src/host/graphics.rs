//! Frame presentation data and synchronous rendering for offscreen readback.

mod capture;
mod commands;
pub mod software;
mod surface;
pub use capture::SequenceCapture;
pub(crate) use commands::triangle_culled;
pub use commands::{
    DrawCommand, DrawList, FrameTexture, FrameTriangle, ProjectedVertex, Rect, TextureAddress,
    TextureFilter, TextureSource,
};

use crate::{Error, Result};
pub use surface::Surface;
pub(crate) use surface::pixel_length;

/// Read-only canvas textures with an explicit upload acknowledgement. A host
/// cache shared by presentation and script readback can consume the dirty flags.
pub struct FrameCanvas<'a> {
    size: [u32; 2],
    tiles: &'a [Option<Surface>],
    dirty: &'a mut [bool],
}

impl<'a> FrameCanvas<'a> {
    pub(crate) fn from_tiles(
        size: [u32; 2],
        tiles: &'a [Option<Surface>],
        dirty: &'a mut [bool],
    ) -> Self {
        Self { size, tiles, dirty }
    }

    /// Logical canvas size in pixels, including unallocated tiles.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Texture slots in canvas order. `None` denotes an unallocated tile.
    pub fn tiles(&self) -> &[Option<Surface>] {
        self.tiles
    }

    /// Upload flags indexed in the same order as `tiles`.
    pub fn dirty_tiles(&self) -> &[bool] {
        self.dirty
    }

    /// Resolve a draw command's texture, including standalone images.
    pub fn texture<'b>(&'b self, source: &'b TextureSource) -> Option<&'b Surface> {
        source.resolve(self.tiles)
    }

    /// Clear dirty flags after uploading all changed tiles to the shared cache.
    pub fn acknowledge_upload(&mut self) {
        self.dirty.fill(false);
    }
}

/// Host renderer for synchronous offscreen readback.
/// The runtime calls it during script updates and when composing active screen
/// filters before giving the final drawing commands to the presentation host.
pub trait GraphicsBackend {
    /// Draw onto a transparent target and return top-down BGRA pixels at `size`.
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut FrameCanvas<'_>,
        size: [u32; 2],
    ) -> Result<Surface>;
}

/// Validate the host's output before exposing it to script or compositor state.
pub(crate) fn render_readback(
    backend: &mut dyn GraphicsBackend,
    frame: &DrawList,
    canvas: &mut FrameCanvas<'_>,
    size: [u32; 2],
) -> Result<Surface> {
    let surface = backend.render(frame, canvas, size)?;
    if surface.size() != size {
        return Err(Error::new(0, "宿主回读尺寸与请求不符"));
    }
    Ok(surface)
}
