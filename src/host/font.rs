use crate::{
    Error, Result,
    message::{GlyphMask, TextEncoding},
};

/// Font selection and metric parameters supplied by the runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontRequest {
    pub encoding: TextEncoding,
    pub name: Vec<u16>,
    pub weight: u32,
    pub italic: bool,
    pub backend: u32,
    /// Character set selector; the original font selection uses its low byte.
    pub glyph_parameter: u32,
    /// Nonzero enables fixed-width layout rules.
    pub measure_parameter: u32,
}

/// Synchronous font selection, metrics, and glyph masks with 64 coverage levels.
pub trait FontServices {
    /// Return horizontal font names in platform enumeration order, retaining
    /// duplicates. `u32::MAX` selects all character sets. Zero pitch selects fixed widths.
    fn enumerate(&mut self, _character_set: u32, _variable_pitch: u32) -> Result<Vec<Vec<u16>>> {
        Err(Error::new(0, "字体服务未提供字体枚举"))
    }

    /// Register a font by CP932 file name. Return false if registration was rejected.
    fn add_resource(&mut self, _name: &[u8]) -> Result<bool> {
        Err(Error::new(0, "字体服务未提供字体资源注册"))
    }

    fn validate(&mut self, request: &FontRequest) -> Result<()>;
    fn measure(&mut self, request: &FontRequest, character: char, size: u32) -> Result<i32>;
    fn rasterize(&mut self, request: &FontRequest, character: char, size: u32)
    -> Result<GlyphMask>;
}
