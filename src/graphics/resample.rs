use super::{Rect, Surface};
use crate::{Error, Result};

/// 目标矩形须落在像素范围内，来源保留采样起点；Y 从底部起算。
/// None 表示即时读取目标自身。
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
    if sw < 0 || sh < 0 || dw <= 0 || dh <= 0 {
        return Ok(false);
    }
    if target.bounds().intersect(to) != Some(to) {
        return Err(Error::new(0, "重采样矩形未裁剪到图像范围"));
    }
    if [sw, sh] == [dw, dh] {
        let Some(clipped) = source.unwrap_or(target).bounds().intersect(from) else {
            return Ok(false);
        };
        let to = Rect::from_xywh(
            to.left,
            to.top,
            clipped.right - clipped.left,
            clipped.bottom - clipped.top,
        );
        let from = clipped;
        for y in 0..to.bottom - to.top {
            let start = bottom_offset(source_size, from.left as usize, (from.top + y) as usize);
            let at = bottom_offset(target.size(), to.left as usize, (to.top + y) as usize);
            let length = (to.right - to.left) as usize * 4;
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
    let step = [sw.wrapping_shl(15) / dw, sh.wrapping_shl(15) / dh];
    if let Some(source) = source.filter(|_| !average) {
        let jobs = (dw as usize * dh as usize / (256 * 1024)).clamp(1, 8);
        let workers = if jobs > 1 && dh > 1 {
            std::thread::available_parallelism().map_or(1, |count| jobs.min(count.get()))
        } else {
            1
        };
        copy_interpolated(target, source, from, to, step, workers);
        return Ok(true);
    }
    let ratio = [sw as f32 / dw as f32, sh as f32 / dh as f32];
    let window = ratio.map(|value| (value as i32).max(1) as usize);
    for y in 0..dh {
        for x in 0..dw {
            let source = source.unwrap_or(target);
            let pixel = if average {
                let at = [
                    from.left.checked_add((x as f32 * ratio[0]) as i32),
                    from.top.checked_add((y as f32 * ratio[1]) as i32),
                ];
                let [Some(sx), Some(sy)] = at else {
                    return Err(Error::new(0, "重采样平均窗口起点溢出"));
                };
                if sx < 0 || sy < 0 {
                    return Err(Error::new(0, "重采样平均窗口起点超出像素存储"));
                }
                let at = [sx as usize, sy as usize];
                average_pixel(source, at, window)?
            } else {
                // 原入口把来源起点直接加到定点累加器，不先左移十五位。
                let at = [
                    from.left.wrapping_add(x.wrapping_mul(step[0])),
                    from.top.wrapping_add(y.wrapping_mul(step[1])),
                ];
                let Some(rows) = InterpolationRow::new(source, at[1]) else {
                    continue;
                };
                let Some(pixel) = rows.sample(at[0]) else {
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

fn copy_interpolated(
    target: &mut Surface,
    source: &Surface,
    from: Rect,
    to: Rect,
    step: [i32; 2],
    workers: usize,
) {
    let stride = target.stride();
    let top = (target.size()[1] as usize - to.bottom as usize) * stride;
    let height = (to.bottom - to.top) as usize;
    let columns = to.left as usize * 4..to.right as usize * 4;
    let output = &mut target.pixels_mut()[top..top + height * stride];
    let render = |first_row: usize, output: &mut [u8]| {
        for (index, row) in output.chunks_exact_mut(stride).enumerate() {
            // 目标行按内存顺序分块，每块从自身底端坐标计算取样相位。
            let y = (height - 1 - first_row - index) as i32;
            let sy = from.top.wrapping_add(y.wrapping_mul(step[1]));
            let Some(source_row) = InterpolationRow::new(source, sy) else {
                continue;
            };
            let mut sx = from.left;
            for pixel in row[columns.clone()].chunks_exact_mut(4) {
                if let Some(sample) = source_row.sample(sx) {
                    pixel.copy_from_slice(&sample);
                }
                sx = sx.wrapping_add(step[0]);
            }
        }
    };
    if workers == 1 {
        render(0, output);
    } else {
        let rows_per_job = height.div_ceil(workers);
        std::thread::scope(|scope| {
            let mut chunks = output.chunks_mut(rows_per_job * stride);
            let first = chunks.next().unwrap();
            for (index, chunk) in chunks.enumerate() {
                scope.spawn(move || render((index + 1) * rows_per_job, chunk));
            }
            render(0, first);
        });
    }
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

struct InterpolationRow<'a> {
    lower: &'a [u8],
    upper: &'a [u8],
    fraction: u32,
}

impl<'a> InterpolationRow<'a> {
    fn new(source: &'a Surface, y: i32) -> Option<Self> {
        let height = source.size()[1] as usize;
        let row = (y >> 15) as usize;
        if row >= height {
            return None;
        }
        let stride = source.stride();
        let bottom = (height - 1 - row) * stride;
        let pixels = source.pixels();
        Some(Self {
            lower: &pixels[bottom..bottom + stride],
            upper: if row + 1 < height {
                &pixels[bottom - stride..bottom]
            } else {
                &[]
            },
            fraction: (y & 0x7fff) as u32,
        })
    }

    fn sample(&self, x: i32) -> Option<[u8; 4]> {
        let offset = ((x >> 15) as usize).wrapping_mul(4);
        let get = |row: &[u8], offset: usize| {
            <[u8; 4]>::try_from(row.get(offset..offset.saturating_add(4))?).ok()
        };
        let first = get(self.lower, offset)?;
        let pixels = [
            first,
            get(self.lower, offset + 4).unwrap_or(first),
            get(self.upper, offset).unwrap_or(first),
            get(self.upper, offset + 4).unwrap_or(first),
        ];
        mix_neighbors(pixels, (x & 0x7fff) as u32, self.fraction)
    }
}

fn mix_neighbors(pixels: [[u8; 4]; 4], fx: u32, fy: u32) -> Option<[u8; 4]> {
    let alpha: u32 = pixels.iter().map(|pixel| u32::from(pixel[3])).sum();
    if alpha == 0 {
        return None;
    }
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

pub(crate) fn flip_rows(surface: &mut Surface) {
    let stride = surface.stride();
    let height = surface.size()[1] as usize;
    for y in 0..height / 2 {
        let (first, last) = surface.pixels_mut().split_at_mut((height - y - 1) * stride);
        first[y * stride..(y + 1) * stride].swap_with_slice(&mut last[..stride]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_rows_preserve_sampling_phase_and_pixels_outside_the_region() {
        let mut source = Surface::new([7, 5]).unwrap();
        for y in 0..5 {
            for x in 0..7 {
                let offset = (y * 7 + x) * 4;
                let alpha = if x < 3 { 0 } else { (y * 52 + x * 7) as u8 };
                source.pixels_mut()[offset..offset + 4].copy_from_slice(&[
                    (x * 33) as u8,
                    (y * 58) as u8,
                    (x * y * 11) as u8,
                    alpha,
                ]);
            }
        }
        let from = Rect::from_xywh(2, 1, 7, 5);
        let to = Rect::from_xywh(3, 2, 18, 23);
        let step = [(7 << 15) / 18, (5 << 15) / 23];
        let mut expected = Surface::new([27, 29]).unwrap();
        expected.clear(0xa511_2233);
        let mut actual = expected.clone();
        copy_interpolated(&mut actual, &source, from, to, step, 8);
        for y in 0..23 {
            for x in 0..18 {
                let row = InterpolationRow::new(&source, from.top + y * step[1]).unwrap();
                if let Some(pixel) = row.sample(from.left + x * step[0]) {
                    let offset = bottom_offset(
                        expected.size(),
                        (to.left + x) as usize,
                        (to.top + y) as usize,
                    );
                    expected.pixels_mut()[offset..offset + 4].copy_from_slice(&pixel);
                }
            }
        }
        assert_eq!(actual, expected);
    }
}
