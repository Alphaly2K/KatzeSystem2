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
    for y in 0..size[1] {
        for x in 0..size[0] {
            let pixel = |index: usize| {
                let mut point = [x, y];
                point[axis] = (point[axis] + index)
                    .saturating_sub(radius)
                    .min(size[axis] - 1);
                let at = point[1] * source.stride() + point[0] * 4;
                &source.pixels()[at..at + 4]
            };
            let mut mean = [0u32; 3];
            let mut count = 0;
            for index in 0..weights.len() {
                let pixel = pixel(index);
                if pixel[3] != 0 {
                    count += 1;
                    for channel in 0..3 {
                        mean[channel] += u32::from(pixel[channel]);
                    }
                }
            }
            if count == 0 {
                continue;
            }
            for value in &mut mean {
                *value /= count;
            }
            let mut sums = [0u32; 4];
            for (index, weight) in weights.iter().enumerate() {
                let pixel = pixel(index);
                let alpha = u32::from(pixel[3]);
                for channel in 0..3 {
                    let color = match alpha {
                        0 => mean[channel],
                        255 => u32::from(pixel[channel]),
                        _ => {
                            (u32::from(pixel[channel]) * alpha + mean[channel] * (255 - alpha)) >> 8
                        }
                    };
                    sums[channel] += color * weight;
                }
                sums[3] += alpha * weight;
            }
            let at = y * output.stride() + x * 4;
            for (channel, sum) in sums.into_iter().enumerate() {
                let fixed = sum.wrapping_mul(factor);
                output.pixels_mut()[at + channel] = ((fixed >> 24) + ((fixed >> 23) & 1)) as u8;
            }
        }
    }
    Ok(output)
}
