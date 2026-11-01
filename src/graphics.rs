use crate::{Error, Result, read};

mod image;
pub use image::{Image, ImageReference, Images};
mod target;
pub use target::{DrawingContext, RenderTarget};
mod transform;
pub use transform::Transform;
mod drawable;
pub use drawable::{Drawable, Drawables, Vertex};
mod plane;
pub use plane::{GraphicsFactory, PlaneObject};
mod projection;
pub use projection::{ProjectedVertex, Projection};
mod raster;

/// 包含左上边、不包含右下边的像素矩形。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// 端点相加按基线的 i32 回绕，不交换反向边界。
    pub fn from_xywh(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            left: x,
            top: y,
            right: x.wrapping_add(width),
            bottom: y.wrapping_add(height),
        }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let rect = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
    }
}

/// 从上到下紧密排列的 BGRA8 渲染面，Alpha 不预乘。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    size: [u32; 2],
    pixels: Vec<u8>,
}

impl Surface {
    pub fn new(size: [u32; 2]) -> Result<Self> {
        let length = pixel_length(size)?;
        Ok(Self {
            size,
            pixels: vec![0; length],
        })
    }

    /// 接收现有像素的所有权，长度须精确等于 width × height × 4。
    pub fn from_bgra(size: [u32; 2], pixels: Vec<u8>) -> Result<Self> {
        if pixels.len() != pixel_length(size)? {
            return Err(Error::new(0, "渲染面像素长度与尺寸不符"));
        }
        Ok(Self { size, pixels })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn stride(&self) -> usize {
        self.size[0] as usize * 4
    }

    pub fn bounds(&self) -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: self.size[0] as i32,
            bottom: self.size[1] as i32,
        }
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// 读取原始 ARGB DWORD；画面外返回 None。
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.size[0] || y >= self.size[1] {
            return None;
        }
        let offset = y as usize * self.stride() + x as usize * 4;
        Some(u32::from_le_bytes(
            self.pixels[offset..offset + 4].try_into().unwrap(),
        ))
    }

    pub fn clear(&mut self, argb: u32) {
        let pixel = argb.to_le_bytes();
        for target in self.pixels.chunks_exact_mut(4) {
            target.copy_from_slice(&pixel);
        }
    }

    /// 裁剪后直接覆盖像素；返回目标区域是否非空。
    pub fn fill_rect(&mut self, rect: Rect, argb: u32) -> bool {
        let Some(rect) = rect.intersect(self.bounds()) else {
            return false;
        };
        let pixel = argb.to_le_bytes();
        let stride = self.stride();
        for y in rect.top as usize..rect.bottom as usize {
            let start = y * stride + rect.left as usize * 4;
            let end = y * stride + rect.right as usize * 4;
            for target in self.pixels[start..end].chunks_exact_mut(4) {
                target.copy_from_slice(&pixel);
            }
        }
        true
    }

    /// 从底部起算 Y 坐标，适配原图像对象的矩形填充坐标。
    pub fn fill_rect_from_bottom(&mut self, rect: Rect, argb: u32) -> bool {
        let Some(rect) = rect.intersect(self.bounds()) else {
            return false;
        };
        self.fill_rect(
            Rect {
                top: self.size[1] as i32 - rect.bottom,
                bottom: self.size[1] as i32 - rect.top,
                ..rect
            },
            argb,
        )
    }

    /// 复制源区域至指定左上位置，裁剪两端并保留源区域到目标的相对位置。
    pub fn copy_from(&mut self, source: &Self, rect: Rect, at: [i32; 2]) -> bool {
        let Some(region) = self.clip_image(source, rect, at) else {
            return false;
        };
        let count = region.width * 4;
        let stride = self.stride();
        let source_stride = source.stride();
        for row in 0..region.height {
            let start = (region.y + row) * stride + region.x * 4;
            let from = (region.source_y + row) * source_stride + region.source_x * 4;
            self.pixels[start..start + count].copy_from_slice(&source.pixels[from..from + count]);
        }
        true
    }

    /// 使用基线普通 32 位 Alpha 规则绘制，并向全零的左右邻接像素延展边缘 RGB。
    pub fn blend_from(&mut self, source: &Self, rect: Rect, at: [i32; 2], opacity: u8) -> bool {
        let Some(region) = self.clip_blend(source, rect, at) else {
            return false;
        };
        self.blend_region(source, region, opacity);
        true
    }

    fn blend_region(&mut self, source: &Self, region: ImageRegion, opacity: u8) {
        let count = region.width * 4;
        let stride = self.stride();
        let source_stride = source.stride();
        for row in 0..region.height {
            let start = (region.y + row) * stride + region.x * 4;
            let from = (region.source_y + row) * source_stride + region.source_x * 4;
            let input = &source.pixels[from..from + count];
            if region.x > 0 {
                extend_color(&mut self.pixels[start - 4..start], &input[..4]);
            }
            for (target, pixel) in self.pixels[start..start + count]
                .chunks_exact_mut(4)
                .zip(input.chunks_exact(4))
            {
                blend_pixel(target, pixel, opacity);
            }
            if region.x + region.width < self.size[0] as usize {
                extend_color(
                    &mut self.pixels[start + count..start + count + 4],
                    &input[count - 4..],
                );
            }
        }
    }

    fn blend_region_within(&mut self, region: ImageRegion, opacity: u8) {
        let count = region.width * 4;
        let stride = self.stride();
        // 原图像按底端起始的地址向前逐行、逐像素读写，重叠区域会读取先前写入的结果。
        for row in (0..region.height).rev() {
            let start = (region.y + row) * stride + region.x * 4;
            let from = (region.source_y + row) * stride + region.source_x * 4;
            if region.x > 0 {
                let pixel: [u8; 4] = self.pixels[from..from + 4].try_into().unwrap();
                extend_color(&mut self.pixels[start - 4..start], &pixel);
            }
            for column in 0..region.width {
                let source = from + column * 4;
                let target = start + column * 4;
                let pixel: [u8; 4] = self.pixels[source..source + 4].try_into().unwrap();
                blend_pixel(&mut self.pixels[target..target + 4], &pixel, opacity);
            }
            if region.x + region.width < self.size[0] as usize {
                let pixel: [u8; 4] = self.pixels[from + count - 4..from + count]
                    .try_into()
                    .unwrap();
                extend_color(&mut self.pixels[start + count..start + count + 4], &pixel);
            }
        }
    }

    fn clip_blend(&self, source: &Self, rect: Rect, at: [i32; 2]) -> Option<ImageRegion> {
        let target = Rect::from_xywh(
            at[0],
            at[1],
            rect.right.wrapping_sub(rect.left),
            rect.bottom.wrapping_sub(rect.top),
        )
        .intersect(self.bounds())?;
        let source = rect.intersect(source.bounds())?;
        Some(ImageRegion {
            x: target.left as usize,
            y: target.top as usize,
            source_x: source.left as usize,
            source_y: source.top as usize,
            width: (source.right - source.left).min(target.right - target.left) as usize,
            height: (source.bottom - source.top).min(target.bottom - target.top) as usize,
        })
    }

    fn clip_image(&self, source: &Self, rect: Rect, at: [i32; 2]) -> Option<ImageRegion> {
        let clipped = rect.intersect(source.bounds())?;
        let x = i64::from(at[0]) + i64::from(clipped.left) - i64::from(rect.left);
        let y = i64::from(at[1]) + i64::from(clipped.top) - i64::from(rect.top);
        let left = x.max(0);
        let top = y.max(0);
        let right = (x + i64::from(clipped.right - clipped.left)).min(i64::from(self.size[0]));
        let bottom = (y + i64::from(clipped.bottom - clipped.top)).min(i64::from(self.size[1]));
        if left >= right || top >= bottom {
            return None;
        }
        Some(ImageRegion {
            x: left as usize,
            y: top as usize,
            source_x: (i64::from(clipped.left) + left - x) as usize,
            source_y: (i64::from(clipped.top) + top - y) as usize,
            width: (right - left) as usize,
            height: (bottom - top) as usize,
        })
    }
}

struct ImageRegion {
    x: usize,
    y: usize,
    source_x: usize,
    source_y: usize,
    width: usize,
    height: usize,
}

fn extend_color(target: &mut [u8], source: &[u8]) {
    if target == [0; 4] {
        target[..3].copy_from_slice(&source[..3]);
    }
}

fn blend_pixel(target: &mut [u8], source: &[u8], opacity: u8) {
    let alpha = (u32::from(opacity) * (u32::from(source[3]) + 1)) >> 8;
    if alpha == 0 {
        extend_color(target, source);
    } else if alpha == 255 || target[3] == 0 {
        target[..3].copy_from_slice(&source[..3]);
        target[3] = alpha as u8;
    } else if target[3] == 255 {
        for (target, source) in target[..3].iter_mut().zip(source) {
            let difference = i32::from(*source) - i32::from(*target);
            *target = (i32::from(*target) + ((difference * alpha as i32) >> 8)) as u8;
        }
    } else {
        let destination_alpha = u32::from(target[3]);
        let overlap = (alpha * destination_alpha) >> 8;
        let destination_weight = ((256 - alpha) * destination_alpha) >> 8;
        let source_weight = (((256 - destination_alpha) * alpha) >> 8) + overlap;
        let output_alpha = (source_weight + destination_weight) | 1;
        let reciprocal = 65536 / output_alpha;
        for (target, source) in target[..3].iter_mut().zip(source) {
            let sum = u32::from(*source) * source_weight + u32::from(*target) * destination_weight;
            *target = ((sum * reciprocal) >> 16) as u8;
        }
        target[3] = output_alpha as u8;
    }
}

fn pixel_length(size: [u32; 2]) -> Result<usize> {
    if size.iter().any(|&value| value > i32::MAX as u32) {
        return Err(Error::new(0, "渲染面尺寸超出坐标范围"));
    }
    let length = (size[0] as usize)
        .checked_mul(4)
        .and_then(|stride| stride.checked_mul(size[1] as usize))
        .filter(|&length| length <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(0, "渲染面像素长度超过上限"))?;
    Ok(length)
}
