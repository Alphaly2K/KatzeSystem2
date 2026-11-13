use super::{Rect, Surface};
use crate::{Error, Result};

/// 两端矩形已经独立裁剪，Y 从底部起算；None 表示即时读取目标自身。
pub(crate) fn copy_scaled(
    target: &mut Surface,
    source: Option<&Surface>,
    from: Rect,
    to: Rect,
) -> Result<bool> {
    let source_size = source.unwrap_or(target).size();
    let extent = |rect: Rect| {
        [
            rect.right.wrapping_sub(rect.left),
            rect.bottom.wrapping_sub(rect.top),
        ]
    };
    let [sw, sh] = extent(from);
    let [dw, dh] = extent(to);
    if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return Ok(false);
    }
    if source.unwrap_or(target).bounds().intersect(from) != Some(from)
        || target.bounds().intersect(to) != Some(to)
    {
        return Err(Error::new(0, "重采样矩形未裁剪到图像范围"));
    }
    if [sw, sh] == [dw, dh] {
        for y in 0..dh {
            let start = bottom_offset(source_size, from.left as usize, (from.top + y) as usize);
            let at = bottom_offset(target.size(), to.left as usize, (to.top + y) as usize);
            let length = dw as usize * 4;
            if let Some(source) = source {
                target.pixels_mut()[at..at + length]
                    .copy_from_slice(&source.pixels()[start..start + length]);
            } else {
                target.pixels_mut().copy_within(start..start + length, at);
            }
        }
        return Ok(true);
    }
    let average = sw > dw.wrapping_mul(4) || sh > dh.wrapping_mul(4);
    let ratio = [sw as f32 / dw as f32, sh as f32 / dh as f32];
    let window = ratio.map(|value| (value as i32).max(1) as usize);
    let step = [sw.wrapping_shl(15) / dw, sh.wrapping_shl(15) / dh];
    for y in 0..dh {
        for x in 0..dw {
            let source = source.unwrap_or(target);
            let pixel = if average {
                let at = [
                    from.left as usize + (x as f32 * ratio[0]) as usize,
                    from.top as usize + (y as f32 * ratio[1]) as usize,
                ];
                average_pixel(source, at, window)?
            } else {
                // 原入口把来源起点直接加到定点累加器，不先左移十五位。
                let at = [
                    from.left.wrapping_add(x.wrapping_mul(step[0])),
                    from.top.wrapping_add(y.wrapping_mul(step[1])),
                ];
                let Some(pixel) = interpolate(source, at) else {
                    continue;
                };
                pixel
            };
            let at = bottom_offset(target.size(), (to.left + x) as usize, (to.top + y) as usize);
            target.pixels_mut()[at..at + 4].copy_from_slice(&pixel);
        }
    }
    Ok(true)
}

fn average_pixel(source: &Surface, at: [usize; 2], window: [usize; 2]) -> Result<[u8; 4]> {
    let size = source.size().map(|value| value as usize);
    let mut sum = [0u32; 4];
    let mut count = 0;
    for y in 0..window[1].min(size[1]) {
        for x in 0..window[0].min(size[0]) {
            // 原窗口以完整图像尺寸限制相对下标，跨行读取仍落在同一像素存储中。
            let index = (at[1] + y) * size[0] + at[0] + x;
            if index >= size[0] * size[1] {
                return Err(Error::new(0, "重采样平均窗口超出像素存储"));
            }
            let offset = bottom_offset(source.size(), index % size[0], index / size[0]);
            for (sum, value) in sum.iter_mut().zip(&source.pixels()[offset..offset + 4]) {
                *sum = sum.wrapping_add(u32::from(*value));
            }
            count += 1;
        }
    }
    Ok(sum.map(|value| (value / count) as u8))
}

fn interpolate(source: &Surface, at: [i32; 2]) -> Option<[u8; 4]> {
    let [x, y] = at.map(|value| (value >> 15) as u32);
    let [width, height] = source.size();
    if x >= width || y >= height {
        return None;
    }
    let get = |x, y| {
        let offset = bottom_offset(source.size(), x as usize, y as usize);
        <[u8; 4]>::try_from(&source.pixels()[offset..offset + 4]).unwrap()
    };
    let first = get(x, y);
    let pixels = [
        first,
        if x + 1 < width { get(x + 1, y) } else { first },
        if y + 1 < height { get(x, y + 1) } else { first },
        if x + 1 < width && y + 1 < height {
            get(x + 1, y + 1)
        } else {
            first
        },
    ];
    let alpha: u32 = pixels.iter().map(|pixel| u32::from(pixel[3])).sum();
    if alpha == 0 {
        return None;
    }
    let [fx, fy] = at.map(|value| (value & 0x7fff) as u32);
    let weights = [
        ((32768 - fx) * (32768 - fy)) >> 15,
        (fx * (32768 - fy)) >> 15,
        ((32768 - fx) * fy) >> 15,
        (fx * fy) >> 15,
    ];
    let mut result = std::array::from_fn(|channel| {
        (pixels
            .iter()
            .zip(weights)
            .map(|(pixel, weight)| u32::from(pixel[channel]) * weight)
            .sum::<u32>()
            >> 15) as u8
    });
    if alpha == 1020 {
        result[3] = 255;
    }
    Some(result)
}

fn bottom_offset(size: [u32; 2], x: usize, y: usize) -> usize {
    ((size[1] as usize - 1 - y) * size[0] as usize + x) * 4
}

pub(super) fn flip_rows(surface: &mut Surface) {
    let stride = surface.stride();
    let height = surface.size()[1] as usize;
    for y in 0..height / 2 {
        let (first, last) = surface.pixels_mut().split_at_mut((height - y - 1) * stride);
        first[y * stride..(y + 1) * stride].swap_with_slice(&mut last[..stride]);
    }
}
