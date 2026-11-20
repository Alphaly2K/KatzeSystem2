//! Character model playback delegated to the host.

use super::graphics::Surface;
use crate::Result;
use std::sync::Arc;

pub trait ModelServices {
    /// Open a model and its options from a script selection in the original byte encoding.
    /// Return `None` if the model format or asset is unavailable.
    fn open(&mut self, selection: &[u8]) -> Result<Option<Box<dyn ModelPlayer>>>;
}

pub trait ModelPlayer {
    /// Sample the model at the host's session timestamp. `None` hides its current frame.
    fn frame(&mut self, system_time_ms: u32) -> Result<Option<ModelFrame>>;
}

pub struct ModelFrame {
    pub pixels: Arc<Surface>,
    /// Top-left of the image relative to the model anchor, in logical pixels.
    pub origin: [i32; 2],
}
