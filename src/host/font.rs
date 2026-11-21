//! Font discovery, metrics and glyph rasterization.

use crate::{Error, Result, read};

mod catalog;
mod face;
pub use catalog::Fonts;
pub use face::Font;

/// The engine text path that produced a font request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Cp932,
    Unicode,
}

/// Top-down glyph coverage, with one sample in `0..=64` per pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphMask {
    size: [u32; 2],
    offset: [i32; 2],
    advance: i32,
    coverage: Vec<u8>,
}

impl GlyphMask {
    /// `offset` is relative to the top-left of the layout entry; `advance`
    /// is the horizontal advance in pixels.
    pub fn new(size: [u32; 2], offset: [i32; 2], advance: i32, coverage: Vec<u8>) -> Result<Self> {
        let length = mask_length(size)?;
        if coverage.len() != length {
            return Err(Error::new(0, "字形遮罩长度与尺寸不符"));
        }
        if let Some(index) = coverage.iter().position(|&value| value > 64) {
            return Err(Error::new(index, "字形灰度超过六十四级"));
        }
        Ok(Self {
            size,
            offset,
            advance,
            coverage,
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn offset(&self) -> [i32; 2] {
        self.offset
    }

    pub fn advance(&self) -> i32 {
        self.advance
    }

    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }
}

pub(crate) fn mask_length(size: [u32; 2]) -> Result<usize> {
    if size.iter().any(|&value| value > i32::MAX as u32) {
        return Err(Error::new(0, "字形尺寸超出支持范围"));
    }
    (size[0] as usize)
        .checked_mul(size[1] as usize)
        .filter(|&length| length <= read::MAX_BYTES / 4)
        .ok_or_else(|| Error::new(0, "字形像素超过存储上限"))
}

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

impl FontRequest {
    /// 将平台整数度量换算为布局字宽；两种编码均以 CP932 双字节映射区分全角。
    pub fn character_advance(&self, character: char, size: u32, measured: i32) -> Result<i32> {
        if size > i32::MAX as u32 {
            return Err(Error::new(0, "字号超过字形服务支持范围"));
        }
        if self.measure_parameter == 0 {
            return Ok(measured);
        }
        let mut text = [0; 4];
        let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(character.encode_utf8(&mut text));
        Ok(if !invalid && encoded.len() == 2 {
            size as i32
        } else {
            measured.max(size.div_ceil(2) as i32)
        })
    }
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
