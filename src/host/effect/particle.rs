use crate::Result;
use crate::host::graphics::TextureAddress;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub rhw: f32,
    pub color: u32,
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleSource {
    pub color: u32,
    pub position_2d: [f32; 2],
    pub position_3d: [f32; 3],
}

impl Default for ParticleVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rhw: 1.0,
            color: u32::MAX,
            uv: [0.0; 2],
        }
    }
}

/// Script and scene values for the current particle update.
pub struct ParticleContext<'a> {
    pub parameters: &'a [u32; 32],
    pub surface_size: [u32; 2],
    pub texture_size: [f32; 2],
    pub depth_offset: f32,
    pub camera_depth: f32,
    pub source: Option<ParticleSource>,
    /// Logical ticks since the first update after `start`.
    pub elapsed: u32,
    /// Logical ticks since the previous update. This is zero on start and one
    /// on the first update.
    pub delta: u32,
}

/// Mesh storage owned by the runtime. All three slices have the same length.
pub struct ParticleFrame<'a> {
    pub quads: &'a mut [[ParticleVertex; 4]],
    pub offsets: &'a mut [[f32; 4]],
    /// A zero entry hides the corresponding quad.
    pub flags: &'a mut [u32],
    pub blend_mode: &'a mut u32,
    pub texture_address: &'a mut [TextureAddress; 2],
    /// Clearing this flag hides the effect and completes its script wait.
    pub active: &'a mut bool,
}

/// Produces mesh updates for one particle task. Presentation stays with the
/// graphics host that consumes the session's draw list.
pub trait ParticlePlayer {
    /// Restart from the initial state. The runtime has reset the mesh.
    fn start(&mut self, context: ParticleContext<'_>, frame: ParticleFrame<'_>) -> Result<()>;
    fn advance(&mut self, context: ParticleContext<'_>, frame: ParticleFrame<'_>) -> Result<()>;
}
