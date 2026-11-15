use super::Image;
use crate::graphics::Surface;
use crate::{Error, Result};

impl Image {
    /// 扩边后依次作水平、垂直三角滤波；半径按有符号值钳制到 0～255。
    pub fn blur(&mut self, radius: [i32; 2]) -> Result<bool> {
        let radius = radius.map(|value| value.clamp(0, 255));
        let offset = self.info.offset;
        if !self.expand_for_blur(radius)? {
            return Ok(false);
        }
        for (axis, radius) in radius.into_iter().enumerate() {
            if radius != 0 {
                self.surface = blur_axis(&self.surface, axis, radius as usize)?;
            }
        }
        for (axis, old) in offset.into_iter().enumerate() {
            self.info.base[axis] =
                self.info.base[axis].wrapping_add(old.wrapping_sub(self.info.offset[axis]));
        }
        self.filter_revision = self.filter_revision.wrapping_add(1);
        Ok(true)
    }

    pub(crate) fn expand_for_blur(&mut self, radius: [i32; 2]) -> Result<bool> {
        if radius.iter().any(|&value| value < 0) || self.surface.pixels().is_empty() {
            return Ok(false);
        }
        let mut before = [0; 2];
        let mut size = self.info.size;
        for axis in 0..2 {
            let offset = u32::try_from(self.info.offset[axis])
                .map_err(|_| Error::new(0, "图像扩边的偏移位于画布之外"))?;
            let remaining = self.info.canvas_size[axis]
                .checked_sub(offset)
                .and_then(|value| value.checked_sub(size[axis]))
                .ok_or_else(|| Error::new(0, "图像扩边的像素范围位于画布之外"))?;
            before[axis] = (radius[axis] as u32).min(offset);
            size[axis] += before[axis] + (radius[axis] as u32).min(remaining);
        }
        if size == self.info.size {
            return Ok(true);
        }
        let mut surface = Surface::new(size)?;
        let top = size[1] - self.info.size[1] - before[1];
        for y in 0..self.info.size[1] as usize {
            let from = y * self.surface.stride();
            let to = (y + top as usize) * surface.stride() + before[0] as usize * 4;
            let length = self.surface.stride();
            surface.pixels_mut()[to..to + length]
                .copy_from_slice(&self.surface.pixels()[from..from + length]);
        }
        self.surface = surface;
        self.info.size = size;
        for (axis, amount) in before.into_iter().enumerate() {
            self.info.offset[axis] -= amount as i32;
        }
        Ok(true)
    }
}

#[derive(Clone, Copy, Default)]
struct LineSum {
    colors: [u32; 3],
    visible: u32,
    opaque: u32,
}

fn rounded(sum: u32, factor: u32) -> u8 {
    let fixed = sum.wrapping_mul(factor);
    ((fixed >> 24) + ((fixed >> 23) & 1)) as u8
}

fn blur_axis(source: &Surface, axis: usize, radius: usize) -> Result<Surface> {
    let size = source.size().map(|value| value as usize);
    let mut output = if axis == 0 {
        Surface::new(source.size())?
    } else {
        source.clone()
    };
    let weights: Vec<_> = (0..=radius * 2)
        .map(|index| {
            let distance = index.abs_diff(radius);
            ((radius + 1 - distance) * 256 + radius / 2) as u32 / (radius + 1) as u32
        })
        .collect();
    let factor = 65536 / (radius + 1) as u32;
    let slope = weights[0] as i32;
    // 整数权重由线性三角核及少量同心窗口修正组成，保留量化后的每项权重。
    let corrections: Vec<_> = (0..=radius)
        .filter_map(|distance| {
            let current =
                weights[radius + distance] as i32 - slope * (radius + 1 - distance) as i32;
            let next = if distance == radius {
                0
            } else {
                weights[radius + distance + 1] as i32 - slope * (radius - distance) as i32
            };
            (current != next).then_some((distance, current - next))
        })
        .collect();
    let opaque_alpha = weights.iter().sum::<u32>() * 255;
    let (lines, length, line_stride, pixel_stride) = if axis == 0 {
        (size[1], size[0], source.stride(), 4)
    } else {
        (size[0], size[1], 4, source.stride())
    };
    // 分块限制细长图像的临时行缓存，边缘仍从完整来源钳制取样。
    let capacity = length.min(1024) + radius * 2;
    let mut pixels = vec![[0u8; 4]; capacity];
    let mut prefix = vec![LineSum::default(); capacity + 1];
    let width = radius + 1;
    for line in 0..lines {
        let base = line * line_stride;
        for start in (0..length).step_by(1024) {
            let count = (length - start).min(1024);
            let padded = count + radius * 2;
            prefix[0] = LineSum::default();
            for i in 0..padded {
                let point = (start + i).saturating_sub(radius).min(length - 1);
                let at = base + point * pixel_stride;
                let pixel: [u8; 4] = source.pixels()[at..at + 4].try_into().unwrap();
                pixels[i] = pixel;
                let mut sum = prefix[i];
                if pixel[3] != 0 {
                    sum.visible += 1;
                    for (channel, color) in pixel[..3].iter().enumerate() {
                        sum.colors[channel] += u32::from(*color);
                    }
                }
                sum.opaque += u32::from(pixel[3] == 255);
                prefix[i + 1] = sum;
            }
            let mut triangle = [0u32; 3];
            for i in 0..width {
                for (channel, sum) in triangle.iter_mut().enumerate() {
                    *sum += prefix[i + width].colors[channel] - prefix[i].colors[channel];
                }
            }
            for x in 0..count {
                let left = prefix[x];
                let right = prefix[x + weights.len()];
                let visible = right.visible - left.visible;
                if visible != 0 {
                    let sums = if right.opaque - left.opaque == weights.len() as u32 {
                        let mut sums = triangle.map(|value| value as i32 * slope);
                        for &(distance, coefficient) in &corrections {
                            let left = prefix[x + radius - distance];
                            let right = prefix[x + radius + distance + 1];
                            for (channel, sum) in sums.iter_mut().enumerate() {
                                *sum += (right.colors[channel] - left.colors[channel]) as i32
                                    * coefficient;
                            }
                        }
                        [sums[0] as u32, sums[1] as u32, sums[2] as u32, opaque_alpha]
                    } else {
                        let mean: [u32; 3] =
                            std::array::from_fn(|i| (right.colors[i] - left.colors[i]) / visible);
                        mixed_sum(&pixels[x..x + weights.len()], &weights, mean)
                    };
                    let at = base + (start + x) * pixel_stride;
                    for (channel, sum) in sums.into_iter().enumerate() {
                        output.pixels_mut()[at + channel] = rounded(sum, factor);
                    }
                }
                if x + 1 < count {
                    for (channel, sum) in triangle.iter_mut().enumerate() {
                        *sum = *sum
                            + (prefix[x + width * 2].colors[channel]
                                - prefix[x + width].colors[channel])
                            - (prefix[x + width].colors[channel] - prefix[x].colors[channel]);
                    }
                }
            }
        }
    }
    Ok(output)
}

// 单独编译混合窗口的归约，保持不透明窗口快速路径的性能。
#[inline(never)]
fn mixed_sum(pixels: &[[u8; 4]], weights: &[u32], mean: [u32; 3]) -> [u32; 4] {
    let mut sums = [0u32; 4];
    for (pixel, weight) in pixels.iter().zip(weights) {
        let alpha = u32::from(pixel[3]);
        // 两个端点直接保留原色或平均色，其余 Alpha 继续使用 255 份混合及截断。
        let foreground = alpha + u32::from(alpha == 255);
        let background = 255 - alpha + u32::from(alpha == 0);
        for channel in 0..3 {
            let color = (u32::from(pixel[channel]) * foreground + mean[channel] * background) >> 8;
            sums[channel] += color * weight;
        }
        sums[3] += alpha * weight;
    }
    sums
}
