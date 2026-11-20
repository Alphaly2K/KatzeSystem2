//! Character model playback delegated to the host.

mod config;
mod selection;
pub use config::{ModelBody, ModelConfig, ModelMotion};
pub use selection::{ModelSelection, ModelSource};

use super::{graphics::Surface, resources::ResourceServices};
use crate::{Error, Result, read};
use std::sync::Arc;

pub trait ModelServices {
    /// Open a model and its options from a script selection in the original byte encoding.
    /// Return `None` if the model format or asset is unavailable.
    fn open(&mut self, _selection: &[u8]) -> Result<Option<Box<dyn ModelPlayer>>> {
        Ok(None)
    }

    /// Open a model through the same resource resolver used by the session.
    /// This is called when a resource service is bound. The default delegates
    /// to [`Self::open`] for hosts that manage model resources separately.
    fn open_with_resources(
        &mut self,
        selection: &[u8],
        _resources: &mut dyn ResourceServices,
    ) -> Result<Option<Box<dyn ModelPlayer>>> {
        self.open(selection)
    }
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

/// Unpack a PSZ model resource for a host decoder. `limit` caps the decoded
/// buffer in bytes. The returned bytes retain the original PSB container.
pub fn decode_psz(bytes: &[u8], limit: usize) -> Result<Vec<u8>> {
    let header = read::range(bytes, 0, 16)?;
    if &header[..4] != b"PSZ\0" {
        return Err(Error::new(0, "模型资源缺少 PSZ 标记"));
    }
    let packed = &bytes[16..];
    if read::u32_at(header, 4)? as usize != packed.len() {
        return Err(Error::new(4, "PSZ 压缩长度与资源不匹配"));
    }
    let expected = read::u32_at(header, 8)? as usize;
    let payload = read::inflate_bounded(packed, expected, limit)?;
    if !payload.starts_with(b"PSB\0") {
        return Err(Error::new(16, "PSZ 解包结果缺少 PSB 标记"));
    }
    Ok(payload)
}
