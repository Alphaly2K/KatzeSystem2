use super::{ProjectedVertex, Rect, Surface};
use crate::graphics::RenderTarget;
use std::sync::Arc;

/// A tile from the frame canvas or an immutable standalone texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSource {
    Canvas(u32),
    Image(Arc<Surface>),
}

impl TextureSource {
    pub fn surface<'a>(&'a self, canvas: &'a RenderTarget) -> Option<&'a Surface> {
        match self {
            Self::Canvas(tile) => canvas.tiles().get(*tile as usize).and_then(Option::as_ref),
            Self::Image(image) => Some(image),
        }
    }
}

/// Sampling outside the normalized texture domain, selected independently for U and V.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureAddress {
    #[default]
    Mirror,
    Wrap,
}

/// Filter for sampling a single texture level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFilter {
    Point,
    Linear,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameTexture {
    pub source: TextureSource,
    pub uv: [[f32; 2]; 3],
    pub address: [TextureAddress; 2],
    /// `None` inherits the draw list's filter.
    pub filter: Option<TextureFilter>,
}

/// One projected triangle in presentation order.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCommand {
    pub vertices: [ProjectedVertex; 3],
    pub texture: Option<FrameTexture>,
    pub clip: Option<Rect>,
    /// Modes 0 through 5 select normal, additive, premultiplied,
    /// multiply, inverse destination, and multiply-add blending.
    /// Other values select normal blending.
    pub blend_mode: u32,
}

pub type FrameTriangle = DrawCommand;

/// Ordered frame commands with no device resources or script object handles.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub commands: Vec<DrawCommand>,
    pub filter: TextureFilter,
}
