use std::collections::BTreeMap;

use super::Layout;
use crate::{
    Error, Result,
    graphics::{Rect, Surface},
    read,
};

/// 一个字形在消息面中的位置、局部绘制范围和跨行渐显起点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevealGlyph {
    pub position: [i32; 2],
    pub bounds: Rect,
    pub start: i32,
}

/// 从左到右显示文字的过渡表；来源保存完整字形，目标保留逐次更新的 Alpha。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reveal {
    gradient: Vec<u8>,
}

impl Reveal {
    pub fn new(width: u32) -> Result<Self> {
        if width == 0 || width as usize > read::MAX_BYTES {
            return Err(Error::new(0, "文字渐显宽度须非零且不超过存储上限"));
        }
        Ok(Self {
            gradient: (0..width)
                .map(|i| !((i.wrapping_mul(255) / width) as u8))
                .collect(),
        })
    }

    pub fn width(&self) -> u32 {
        self.gradient.len() as u32
    }

    pub fn gradient(&self) -> &[u8] {
        &self.gradient
    }

    /// 此路径使用原始时刻、普通字号和字间隔，不使用普通播放的速度基准。
    pub fn phase(now: u32, normal_size: u32, character_delay: u32) -> i32 {
        if now >= 0x8000_0000 || character_delay == 0 {
            i32::MAX
        } else {
            (now.wrapping_mul(normal_size).wrapping_mul(2) / character_delay) as i32
        }
    }

    /// 新字形写入来源后，同步整面的 RGB，目标 Alpha 保持。
    pub fn copy_colors(target: &mut Surface, source: &Surface) -> Result<()> {
        equal_size(target, source)?;
        for (to, from) in target
            .pixels_mut()
            .chunks_exact_mut(4)
            .zip(source.pixels().chunks_exact(4))
        {
            to[..3].copy_from_slice(&from[..3]);
        }
        Ok(())
    }

    /// 只更新范围内的 Alpha，返回该字形是否完成；时间回退时保留未触及的像素。
    pub fn apply(
        &self,
        target: &mut Surface,
        source: &Surface,
        glyph: &RevealGlyph,
        phase: i32,
    ) -> Result<bool> {
        equal_size(target, source)?;
        let [width, height] = target.size();
        if width == 0 || height == 0 {
            return Err(Error::new(0, "文字渐显需要非零尺寸的渲染面"));
        }
        if phase < glyph.start {
            return Ok(false);
        }
        let left = (glyph.position[0].wrapping_add(glyph.bounds.left) as u32).min(width - 1);
        let right = (glyph.position[0].wrapping_add(glyph.bounds.right) as u32).min(width - 1);
        let top = (glyph.position[1].wrapping_add(glyph.bounds.top) as u32).min(height - 1);
        let bottom = (glyph.position[1].wrapping_add(glyph.bounds.bottom) as u32).min(height - 1);
        let phase = phase.wrapping_sub(self.width() as i32);
        let complete = phase >= glyph.bounds.right.wrapping_add(glyph.start);
        let stride = target.stride();
        // 基线逐端点作无符号限制，限制后的右、下端点仍不写入。
        for y in top..bottom {
            for x in left..right {
                let index = y as usize * stride + x as usize * 4 + 3;
                let relative = (x as i32)
                    .wrapping_sub(glyph.position[0])
                    .wrapping_sub(phase)
                    .wrapping_add(glyph.start);
                if complete || relative < 0 {
                    target.pixels_mut()[index] = source.pixels()[index];
                } else if let Some(&weight) = self.gradient.get(relative as usize) {
                    target.pixels_mut()[index] =
                        (((u16::from(weight) + 1) * u16::from(source.pixels()[index])) >> 8) as u8;
                }
            }
        }
        Ok(complete)
    }
}

fn equal_size(target: &Surface, source: &Surface) -> Result<()> {
    if target.size() != source.size() {
        Err(Error::new(0, "文字渐显的来源与目标尺寸不符"))
    } else {
        Ok(())
    }
}

impl Layout {
    /// 按行串联字形的水平距离；控制记录为 None，内部控制码九重置累计距离。
    pub fn reveal_starts(&self) -> Vec<Option<i32>> {
        let mut lines: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        let reset = self.entries.iter().position(|entry| entry.code() == 9);
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.glyph() {
                lines.entry(entry.line).or_default().push(index);
            }
        }
        let mut result = vec![None; self.entries.len()];
        let mut accumulated = 0i32;
        let mut previous_line = 0u32;
        for (line, indices) in lines {
            if reset.is_some() && line > previous_line.saturating_add(1) {
                accumulated = 0;
            }
            let left = indices
                .iter()
                .map(|&i| self.entries[i].position[0] as u32)
                .min()
                .unwrap() as i32;
            for &index in &indices {
                let base = if reset.is_some_and(|r| r < index) {
                    0
                } else {
                    accumulated
                };
                result[index] = Some(
                    self.entries[index].position[0]
                        .wrapping_sub(left)
                        .wrapping_add(base),
                );
            }
            let last = &self.entries[*indices.last().unwrap()];
            accumulated = (if reset.is_some() { 0 } else { accumulated })
                .wrapping_add(last.position[0])
                .wrapping_add(last.size[0])
                .wrapping_sub(left);
            previous_line = line;
        }
        result
    }
}
