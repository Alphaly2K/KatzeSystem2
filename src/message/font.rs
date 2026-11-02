use super::message_color_argb;
use crate::{Error, Result, graphics::Surface, read};

/// 顶端起始的六十四级字形覆盖；offset 相对于布局记录左上位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphMask {
    size: [u32; 2],
    offset: [i32; 2],
    advance: i32,
    coverage: Vec<u8>,
}

impl GlyphMask {
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

    /// 输入颜色使用消息的 AABBGGRR；覆盖决定 Alpha，颜色自身的 Alpha 不参与。
    pub fn colorize(&self, color: u32, double_coverage: bool) -> Result<Surface> {
        let color = message_color_argb(color).to_le_bytes();
        let mut pixels = Vec::with_capacity(self.coverage.len() * 4);
        for &sample in &self.coverage {
            let alpha = u32::from(sample) * 255 / 64;
            let alpha = if double_coverage {
                (alpha * 2).min(255)
            } else {
                alpha
            };
            pixels.extend_from_slice(&[color[0], color[1], color[2], alpha as u8]);
        }
        Surface::from_bgra(self.size, pixels)
    }
}

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
        let metrics = self.metrics(character, size)?;
        Ok(metrics.advance_width.round() as i32)
    }

    pub fn rasterize(&self, character: char, size: u32) -> Result<GlyphMask> {
        self.metrics(character, size)?;
        let (metrics, coverage) = self.font.rasterize(character, size as f32);
        let baseline = self
            .font
            .horizontal_line_metrics(size as f32)
            .map_or(size as i32, |line| line.ascent.round() as i32)
            .min(size as i32);
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

    fn metrics(&self, character: char, size: u32) -> Result<fontdue::Metrics> {
        if !self.has_character(character) {
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

fn mask_length(size: [u32; 2]) -> Result<usize> {
    if size.iter().any(|&value| value > i32::MAX as u32) {
        return Err(Error::new(0, "字形尺寸超出支持范围"));
    }
    let length = (size[0] as usize)
        .checked_mul(size[1] as usize)
        .filter(|&length| length <= read::MAX_BYTES / 4)
        .ok_or_else(|| Error::new(0, "字形像素超过存储上限"))?;
    Ok(length)
}
