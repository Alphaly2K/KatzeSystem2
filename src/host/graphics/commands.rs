use super::Surface;
use std::sync::Arc;

/// Projected position, reciprocal W for interpolation, and packed ARGB color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedVertex {
    pub position: [f32; 3],
    pub rhw: f32,
    pub color: u32,
}

/// A pixel rectangle with inclusive left/top and exclusive right/bottom edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// Endpoints use wrapping addition. Negative extents are not normalized.
    pub fn from_xywh(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            left: x,
            top: y,
            right: x.wrapping_add(width),
            bottom: y.wrapping_add(height),
        }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let rect = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
    }
}

/// A tile from the frame canvas or an immutable standalone texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSource {
    Canvas(u32),
    Image(Arc<Surface>),
}

impl TextureSource {
    pub(crate) fn resolve<'a>(&'a self, tiles: &'a [Option<Surface>]) -> Option<&'a Surface> {
        match self {
            Self::Canvas(tile) => tiles.get(*tile as usize).and_then(Option::as_ref),
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

impl DrawCommand {
    /// Test whether projection or subpixel coverage leaves no triangle to draw.
    pub fn is_culled(&self) -> bool {
        triangle_culled(self.vertices)
    }
}

pub(crate) fn triangle_culled(vertices: [ProjectedVertex; 3]) -> bool {
    if vertices.iter().all(|v| v.rhw < 0.)
        || vertices
            .iter()
            .all(|v| v.position[2].is_finite() && v.position[2] < 0.)
        || vertices
            .iter()
            .all(|v| v.position[2].is_finite() && v.position[2] > 1.)
        || vertices
            .iter()
            .any(|v| v.position[..2].iter().any(|n| !n.is_finite()))
    {
        return true;
    }
    let points = vertices.map(|v| {
        [v.position[0], v.position[1]].map(|n| (f64::from(n) * 256. + 0.5).floor() / 256.)
    });
    let [a, b, c] = points;
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    area == 0.
}

/// Ordered frame commands with no device resources or script object handles.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub commands: Vec<DrawCommand>,
    pub filter: TextureFilter,
}
