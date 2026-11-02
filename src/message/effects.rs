use crate::{Error, Result, graphics::Surface};

/// 字形效果按轮廓、偏移阴影的顺序执行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphEffects {
    pub outline: bool,
    pub shadow: bool,
    pub shadow_offset: [i32; 2],
}

impl Default for GlyphEffects {
    fn default() -> Self {
        Self {
            outline: false,
            shadow: false,
            shadow_offset: [1; 2],
        }
    }
}

impl GlyphEffects {
    /// 来源与目标须同尺寸。返回是否启用效果；不清空目标，也不绘制前景。
    pub fn apply(
        &self,
        target: &mut Surface,
        source: &Surface,
        font_size: u32,
        color: u32,
    ) -> Result<bool> {
        if !self.outline && !self.shadow {
            return Ok(false);
        }
        if source.size() != target.size() {
            return Err(Error::new(0, "字形效果的来源与目标尺寸不符"));
        }
        let [width, height] = source.size().map(|n| n as usize);
        if self.outline && width > height {
            return Err(Error::new(0, "基线轮廓会在宽大于高的渲染面上越界读取"));
        }
        let offset = self.offset(font_size)?;
        if source.pixels().is_empty() {
            return Ok(true);
        }
        // 基线阴影直接写入 AABBGGRR 的低三字节，与前景的红蓝换算分别处理。
        let [first, green, third, _] = color.to_le_bytes();
        let pixel = [first, green, third, 0];
        if self.outline {
            outline(target, source, pixel);
        }
        if self.shadow {
            shadow(target, source, offset, pixel);
        }
        Ok(true)
    }

    fn offset(&self, font_size: u32) -> Result<[usize; 2]> {
        let offset = self.shadow_offset.map(|value| {
            if font_size < 20 {
                value.wrapping_add(1) >> 1
            } else {
                value
            }
        });
        if self.shadow && offset.iter().any(|&value| value < 0) {
            return Err(Error::new(0, "基线负字形阴影偏移会越界访问"));
        }
        Ok(offset.map(|n| n as usize))
    }
}

fn outline(target: &mut Surface, source: &Surface, mut pixel: [u8; 4]) {
    let [width, height] = source.size().map(|n| n as usize);
    let stride = source.stride();
    let mut line = vec![0; height + 2];
    for y in 0..height {
        for x in 0..width {
            line[x + 1] = source.pixels()[y * stride + x * 4 + 3];
        }
        for x in 0..width {
            pixel[3] = sum(&line[x..x + 3]);
            let index = y * stride + x * 4;
            target.pixels_mut()[index..index + 4].copy_from_slice(&pixel);
        }
    }
    line.fill(0);
    for x in 0..width {
        // 基线按宽度复制列，纵向输出仍按高度；高于宽度的尾部读取零缓存。
        for y in 0..width {
            line[y + 1] = target.pixels()[y * stride + x * 4 + 3];
        }
        for y in 0..height {
            pixel[3] = sum(&line[y..y + 3]);
            let index = y * stride + x * 4;
            target.pixels_mut()[index..index + 4].copy_from_slice(&pixel);
        }
    }
}

fn shadow(target: &mut Surface, source: &Surface, offset: [usize; 2], mut pixel: [u8; 4]) {
    let [width, height] = source.size().map(|n| n as usize);
    let stride = source.stride();
    for y in (0..height.saturating_sub(offset[1])).rev() {
        for x in 0..width.saturating_sub(offset[0]) {
            let from = y * stride + x * 4;
            let to = (y + offset[1]) * stride + (x + offset[0]) * 4;
            pixel[3] = sum(&[
                target.pixels()[to + 3],
                target.pixels()[from + 3],
                source.pixels()[from + 3],
            ]);
            target.pixels_mut()[to..to + 4].copy_from_slice(&pixel);
        }
    }
}

fn sum(values: &[u8]) -> u8 {
    values
        .iter()
        .map(|&value| u16::from(value))
        .sum::<u16>()
        .min(255) as u8
}
