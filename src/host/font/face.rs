use super::{GlyphMask, mask_length};
use crate::{Error, Result, read};

/// 从显式字体字节构造的单字体灰度栅格化服务，不查询系统字体或提供缺字回退。
#[derive(Clone, Debug)]
pub struct Font {
    font: fontdue::Font,
}

impl Font {
    pub fn from_bytes(bytes: &[u8], collection_index: u32) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字体数据超过存储上限"));
        }
        let font = fontdue::Font::from_bytes(
            bytes,
            fontdue::FontSettings {
                collection_index,
                load_substitutions: false,
                ..Default::default()
            },
        )
        .map_err(|message| Error::new(0, format!("字体读取失败：{message}")))?;
        Ok(Self { font })
    }

    pub fn has_character(&self, character: char) -> bool {
        self.font.lookup_glyph_index(character) != 0
    }

    pub fn measure(&self, character: char, size: u32) -> Result<i32> {
        let metrics = self.metrics(character, size, false)?;
        Ok(metrics.advance_width.round() as i32)
    }

    pub fn rasterize(&self, character: char, size: u32) -> Result<GlyphMask> {
        self.rasterize_glyph(character, size, false)
    }

    pub(super) fn measure_or_missing(&self, character: char, size: u32) -> Result<i32> {
        Ok(self.metrics(character, size, true)?.advance_width.round() as i32)
    }

    pub(super) fn rasterize_or_missing(&self, character: char, size: u32) -> Result<GlyphMask> {
        self.rasterize_glyph(character, size, true)
    }

    fn rasterize_glyph(
        &self,
        character: char,
        size: u32,
        allow_missing: bool,
    ) -> Result<GlyphMask> {
        self.metrics(character, size, allow_missing)?;
        let (metrics, coverage) = self.font.rasterize(character, size as f32);
        let baseline = self.baseline(size);
        GlyphMask::new(
            [metrics.width as u32, metrics.height as u32],
            [
                metrics.xmin.wrapping_add(1).max(0),
                baseline
                    .wrapping_sub(metrics.ymin)
                    .wrapping_sub(metrics.height as i32)
                    .wrapping_add(1),
            ],
            metrics.advance_width.round() as i32,
            coverage
                .into_iter()
                .map(|value| ((u32::from(value) * 64 + 127) / 255) as u8)
                .collect(),
        )
    }

    pub(super) fn baseline(&self, size: u32) -> i32 {
        self.font
            .horizontal_line_metrics(size as f32)
            .map_or(size as i32, |line| line.ascent.round() as i32)
            .min(size as i32)
    }

    fn metrics(&self, character: char, size: u32, allow_missing: bool) -> Result<fontdue::Metrics> {
        if !allow_missing && !self.has_character(character) {
            return Err(Error::new(
                0,
                format!("字体缺少字符 U+{:04X}", character as u32),
            ));
        }
        if size > i32::MAX as u32 {
            return Err(Error::new(0, "字号超过字形服务支持范围"));
        }
        let metrics = self.font.metrics(character, size as f32);
        if !metrics.advance_width.is_finite()
            || metrics.advance_width.round() < i32::MIN as f32
            || f64::from(metrics.advance_width.round()) > f64::from(i32::MAX)
            || metrics.width > i32::MAX as usize
            || metrics.height > i32::MAX as usize
        {
            return Err(Error::new(0, "字形度量超出支持范围"));
        }
        mask_length([metrics.width as u32, metrics.height as u32])?;
        Ok(metrics)
    }
}
