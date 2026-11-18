use super::Image;
use crate::graphics::Surface;
use crate::{Error, Result};

impl Image {
    /// 扩边后依次作水平、垂直三角滤波；24 位水平分支的未初始化结果返回诊断。
    pub fn blur(&mut self, radius: [i32; 2]) -> Result<bool> {
        if self.is_rgb() {
            return self.blur_rgb(radius);
        }
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
            let (before_radius, after_radius) = if self.is_rgb() {
                // 三通道入口依次传入左、右、下、上，两个半径交错用于扩边。
                (radius[0], radius[1])
            } else {
                (radius[axis], radius[axis])
            };
            before[axis] = (before_radius as u32).min(offset);
            size[axis] += before[axis] + (after_radius as u32).min(remaining);
        }
        if size == self.info.size {
            return Ok(true);
        }
        if self.is_rgb() {
            let old_stride = (self.info.size[0] as usize * 3 + 3) & !3;
            let stride = (size[0] as usize * 3 + 3) & !3;
            let after = size[0] - self.info.size[0] - before[0];
            if old_stride != self.info.size[0] as usize * 3
                && (after != 0 || before[0] as usize * 3 + old_stride > stride)
            {
                return Err(Error::new(0, "24 位扩边会将未初始化的行填充复制到可见像素"));
            }
        }
        let mut surface = Surface::new(size)?;
        if self.is_rgb() {
            surface.clear(0xff00_0000);
        }
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

    fn blur_rgb(&mut self, radius: [i32; 2]) -> Result<bool> {
        if radius.iter().any(|&value| value < 0) || self.surface.pixels().is_empty() {
            return Ok(false);
        }
        if radius[0] != 0 {
            return Err(Error::new(0, "24 位水平模糊会留下未初始化的可见像素"));
        }
        let offset = self.info.offset;
        if !self.expand_for_blur(radius)? {
            return Ok(false);
        }
        let radius = radius[1].min(255) as usize;
        if radius != 0 {
            let source = self.surface.clone();
            let [width, height] = source.size().map(|value| value as usize);
            let factor = 256 / (radius + 1) as u32;
            let weights: Vec<_> = (0..=radius * 2)
                .map(|index| {
                    ((radius + 1 - index.abs_diff(radius)) * 256 + radius / 2) as u32
                        / (radius + 1) as u32
                })
                .collect();
            for y in 0..height {
                for x in 0..width {
                    let mut sums = [0u32; 3];
                    for (index, &weight) in weights.iter().enumerate() {
                        let row = (y + index).saturating_sub(radius).min(height - 1);
                        let at = row * source.stride() + x * 4;
                        for (sum, &color) in sums.iter_mut().zip(&source.pixels()[at..at + 3]) {
                            *sum += u32::from(color) * weight;
                        }
                    }
                    let at = y * self.surface.stride() + x * 4;
                    for (value, sum) in self.surface.pixels_mut()[at..at + 3].iter_mut().zip(sums) {
                        *value = (sum.wrapping_mul(factor) >> 16) as u8;
                    }
                }
            }
        }
        for (axis, old) in offset.into_iter().enumerate() {
            self.info.base[axis] =
                self.info.base[axis].wrapping_add(old.wrapping_sub(self.info.offset[axis]));
        }
        self.filter_revision = self.filter_revision.wrapping_add(1);
        Ok(true)
    }
}

#[derive(Clone, Copy, Default)]
struct LineSum<C> {
    colors: C,
    visible: u32,
    opaque: u32,
}

trait ColorSum: Copy + Default {
    fn from_pixel(pixel: [u8; 4]) -> Self;
    fn channels(self) -> [u32; 3];
    fn add(self, right: Self) -> Self;
    fn sub(self, right: Self) -> Self;
    fn scale(self, factor: i32) -> Self;
}

impl ColorSum for u64 {
    fn from_pixel([b, g, r, _]: [u8; 4]) -> Self {
        u64::from(b) | (u64::from(g) << 21) | (u64::from(r) << 42)
    }

    fn channels(self) -> [u32; 3] {
        std::array::from_fn(|i| ((self >> (21 * i)) & 0x1f_ffff) as u32)
    }

    fn add(self, right: Self) -> Self {
        self.wrapping_add(right)
    }

    fn sub(self, right: Self) -> Self {
        self.wrapping_sub(right)
    }

    fn scale(self, factor: i32) -> Self {
        // 修正系数可为负；所有修正累加后，才解包最终的非负通道值。
        self.wrapping_mul(factor as u64)
    }
}

impl ColorSum for [u32; 3] {
    fn from_pixel([b, g, r, _]: [u8; 4]) -> Self {
        [b.into(), g.into(), r.into()]
    }

    fn channels(self) -> [u32; 3] {
        self
    }

    fn add(self, right: Self) -> Self {
        std::array::from_fn(|i| self[i].wrapping_add(right[i]))
    }

    fn sub(self, right: Self) -> Self {
        std::array::from_fn(|i| self[i].wrapping_sub(right[i]))
    }

    fn scale(self, factor: i32) -> Self {
        self.map(|value| value.wrapping_mul(factor as u32))
    }
}

fn rounded(sum: u32, factor: u32) -> u8 {
    let fixed = sum.wrapping_mul(factor);
    ((fixed >> 24) + ((fixed >> 23) & 1)) as u8
}

fn blur_axis(source: &Surface, axis: usize, radius: usize) -> Result<Surface> {
    let jobs = (source.pixels().len() / (4 * 256 * 1024)).clamp(1, 8);
    let workers = if jobs > 1 {
        std::thread::available_parallelism().map_or(1, |count| jobs.min(count.get()))
    } else {
        1
    };
    blur_axis_with_workers(source, axis, radius, workers)
}

fn blur_axis_with_workers(
    source: &Surface,
    axis: usize,
    radius: usize,
    workers: usize,
) -> Result<Surface> {
    // 至半径 31，最终权重和最多 8192，乘 255 后仍能装进 21 位。
    if radius <= 31 {
        blur_axis_with_sum::<u64>(source, axis, radius, workers)
    } else {
        blur_axis_with_sum::<[u32; 3]>(source, axis, radius, workers)
    }
}

fn blur_axis_with_sum<C: ColorSum>(
    source: &Surface,
    axis: usize,
    radius: usize,
    workers: usize,
) -> Result<Surface> {
    let height = source.size()[1] as usize;
    let mut output = if axis == 0 {
        Surface::new(source.size())?
    } else {
        source.clone()
    };
    if output.pixels().is_empty() {
        return Ok(output);
    }
    if axis == 0 && workers > 1 && height >= workers * 4 {
        // 水平滤波的行彼此独立，交错分配可分散集中在少数行上的混合窗口。
        let rows_per_job = height.div_ceil(workers * 4);
        let bytes_per_job = rows_per_job * output.stride();
        let mut groups: Vec<Vec<_>> = (0..workers).map(|_| Vec::new()).collect();
        for (index, chunk) in output.pixels_mut().chunks_mut(bytes_per_job).enumerate() {
            groups[index % workers].push((index * rows_per_job, chunk));
        }
        let run = |group: Vec<(usize, &mut [u8])>| {
            for (row, output) in group {
                blur_rows::<C>(source, axis, radius, row, output);
            }
        };
        std::thread::scope(|scope| {
            let mut groups = groups.into_iter();
            let first = groups.next().unwrap();
            for group in groups.filter(|group| !group.is_empty()) {
                scope.spawn(move || run(group));
            }
            run(first);
        });
    } else if workers > 1 && height > 1 {
        let rows_per_job = height.div_ceil(workers);
        let bytes_per_job = rows_per_job * output.stride();
        std::thread::scope(|scope| {
            let mut chunks = output.pixels_mut().chunks_mut(bytes_per_job);
            let first = chunks.next().unwrap();
            for (index, chunk) in chunks.enumerate() {
                scope.spawn(move || {
                    blur_rows::<C>(source, axis, radius, (index + 1) * rows_per_job, chunk)
                });
            }
            blur_rows::<C>(source, axis, radius, 0, first);
        });
    } else {
        blur_rows::<C>(source, axis, radius, 0, output.pixels_mut());
    }
    Ok(output)
}

fn blur_rows<C: ColorSum>(
    source: &Surface,
    axis: usize,
    radius: usize,
    first_row: usize,
    output: &mut [u8],
) {
    let [w, h] = source.size().map(|value| value as usize);
    let stride = source.stride();
    let rows = first_row..first_row + output.len() / stride;
    let (lines, span, length, line_stride, pixel_stride) = if axis == 0 {
        (rows, 0..w, w, stride, 4)
    } else {
        (0..w, rows, h, 4, stride)
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
    // 并行分块只限制输出行；取样始终以完整来源图像钳制坐标。
    let capacity = span.len().min(1024) + radius * 2;
    let mut pixels = vec![[0u8; 4]; capacity];
    let mut prefix = vec![LineSum::<C>::default(); capacity + 1];
    let width = radius + 1;
    for line in lines.clone() {
        let base = line * line_stride;
        let target_base = (line - lines.start) * line_stride;
        for start in span.clone().step_by(1024) {
            let count = (span.end - start).min(1024);
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
                    sum.colors = sum.colors.add(C::from_pixel(pixel));
                }
                sum.opaque += u32::from(pixel[3] == 255);
                prefix[i + 1] = sum;
            }
            let total = prefix[padded].visible;
            if total == 0 {
                continue;
            }
            // 首尾没有可见样本的窗口不写输出；垂直轴因此保留原有透明像素。
            let window = weights.len();
            let first = if prefix[window].visible == 0 {
                prefix[window..window + count].partition_point(|sum| sum.visible == 0)
            } else {
                0
            };
            let end = if prefix[count - 1].visible == total {
                prefix[..count].partition_point(|sum| sum.visible < total)
            } else {
                count
            };
            let mut triangle = C::default();
            for i in first..first + width {
                triangle = triangle.add(prefix[i + width].colors.sub(prefix[i].colors));
            }
            for x in first..end {
                let left = prefix[x];
                let right = prefix[x + window];
                let visible = right.visible - left.visible;
                if visible != 0 {
                    let sums = if right.opaque - left.opaque == window as u32 {
                        let mut sum = triangle.scale(slope);
                        for &(distance, coefficient) in &corrections {
                            let left = prefix[x + radius - distance];
                            let right = prefix[x + radius + distance + 1];
                            sum = sum.add(right.colors.sub(left.colors).scale(coefficient));
                        }
                        let [b, g, r] = sum.channels();
                        [b, g, r, opaque_alpha]
                    } else {
                        let mean = right
                            .colors
                            .sub(left.colors)
                            .channels()
                            .map(|v| v / visible);
                        mixed_sum(&pixels[x..x + window], &weights, mean)
                    };
                    let at = target_base + (start - span.start + x) * pixel_stride;
                    for (channel, sum) in sums.into_iter().enumerate() {
                        output[at + channel] = rounded(sum, factor);
                    }
                }
                if x + 1 < end {
                    let middle = prefix[x + width].colors;
                    triangle = triangle
                        .add(prefix[x + width * 2].colors.sub(middle))
                        .sub(middle.sub(prefix[x].colors));
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_ends_keep_axis_defaults_and_filter_across_a_chunk_boundary() {
        for (axis, size) in [(0, [2050, 1]), (1, [1, 2050])] {
            let mut source = Surface::new(size).unwrap();
            let invisible = [17, 33, 65, 0];
            let visible = [50, 120, 210, 255];
            for pixel in source.pixels_mut().chunks_exact_mut(4) {
                pixel.copy_from_slice(&invisible);
            }
            source.pixels_mut()[1024 * 4..1025 * 4].copy_from_slice(&visible);

            let result = blur_axis_with_workers(&source, axis, 3, 1).unwrap();
            for (i, pixel) in result.pixels().chunks_exact(4).enumerate() {
                let distance = i.abs_diff(1024);
                if distance <= 3 {
                    assert_eq!(
                        pixel,
                        [50, 120, 210, [64, 48, 32, 16][distance]],
                        "{axis}, {i}"
                    );
                } else {
                    assert_eq!(
                        pixel,
                        if axis == 0 { [0; 4] } else { invisible },
                        "{axis}, {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn blur_workers_sample_across_output_stripes() {
        for (size, radius) in [
            ([1031, 37], 7),
            ([19, 1033], 16),
            ([71, 83], 18),
            ([1031, 83], 31),
            ([23, 9], 32),
            ([23, 9], 255),
        ] {
            let mut source = Surface::new(size).unwrap();
            for (index, pixel) in source.pixels_mut().chunks_exact_mut(4).enumerate() {
                let x = index as u32 % size[0];
                let y = index as u32 / size[0];
                if x >= 3 && y >= 5 {
                    // 接近通道位宽上限的不透明区域，混合区另保留不同通道颜色。
                    pixel.copy_from_slice(&[255, 253, 251, 255]);
                } else {
                    let alpha = match (x / 5 + y / 13) % 5 {
                        0 | 1 => 0,
                        2 => 127,
                        _ => 255,
                    };
                    pixel.copy_from_slice(&[
                        (3 * x + y) as u8,
                        (index % 239) as u8,
                        y as u8,
                        alpha,
                    ]);
                }
            }
            for axis in 0..2 {
                let single = blur_axis_with_sum::<[u32; 3]>(&source, axis, radius, 1).unwrap();
                let split = blur_axis_with_workers(&source, axis, radius, 8).unwrap();
                assert_eq!(single.pixels(), split.pixels(), "{size:?}, axis {axis}");
            }
        }
    }
}
