use super::{GlyphEffects, message_color_argb};
use crate::{
    Result,
    graphics::{Rect, Surface},
    host::font::GlyphMask,
};

/// 单个字形的颜色、覆盖与效果；font_size 使用记录中的原始字号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphPaint {
    pub color: u32,
    pub shadow_color: u32,
    pub font_size: u32,
    pub double_coverage: bool,
    pub effects: GlyphEffects,
}

impl GlyphMask {
    /// 输入颜色使用消息的 AABBGGRR；覆盖决定 Alpha，颜色自身的 Alpha 不参与。
    pub fn colorize(&self, color: u32, double_coverage: bool) -> Result<Surface> {
        let color = message_color_argb(color).to_le_bytes();
        let mut pixels = Vec::with_capacity(self.coverage().len() * 4);
        for &sample in self.coverage() {
            let alpha = u32::from(sample) * 255 / 64;
            let alpha = if double_coverage {
                (alpha * 2).min(255)
            } else {
                alpha
            };
            pixels.extend_from_slice(&[color[0], color[1], color[2], alpha as u8]);
        }
        Surface::from_bgra(self.size(), pixels)
    }

    /// 将字形置于同尺寸临时面，先写效果再合成前景，返回顶端起始的原始绘制范围。
    pub fn paint(&self, target: &mut Surface, paint: &GlyphPaint) -> Result<Rect> {
        let image = self.colorize(paint.color, paint.double_coverage)?;
        let mut source = Surface::new(target.size())?;
        let [x, y] = self.offset();
        let [width, height] = self.size();
        source.copy_from(&image, image.bounds(), [x, y]);
        let bounds = Rect::from_xywh(
            0,
            0,
            x.wrapping_add(width as i32)
                .wrapping_add(paint.effects.shadow_offset[0])
                .wrapping_add(2),
            y.wrapping_add(height as i32)
                .wrapping_add(paint.effects.shadow_offset[1])
                .wrapping_add(2),
        );
        if paint
            .effects
            .apply(target, &source, paint.font_size, paint.shadow_color)?
        {
            target.blend_from(&source, bounds, [0; 2], 255);
        } else {
            target.copy_from(&source, bounds, [0; 2]);
        }
        Ok(bounds)
    }
}
