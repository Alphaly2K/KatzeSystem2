use super::Rect;
use crate::{Error, Result};

/// 从上到下紧密排列的 BGRA8 渲染面，Alpha 不预乘。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    size: [u32; 2],
    pixels: Vec<u8>,
}

const MAX_SURFACE_BYTES: usize = 256 * 1024 * 1024;

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

    pub(crate) fn adjust_rgb_percent(&mut self, rgb: [u32; 3]) {
        for pixel in self.pixels_mut().chunks_exact_mut(4) {
            for (value, percent) in pixel[..3].iter_mut().zip(rgb.into_iter().rev()) {
                *value = (u32::from(*value).wrapping_mul(percent) / 100).min(255) as u8;
            }
        }
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
}

pub(crate) fn pixel_length(size: [u32; 2]) -> Result<usize> {
    if size.iter().any(|&value| value > i32::MAX as u32) {
        return Err(Error::new(0, "渲染面尺寸超出坐标范围"));
    }
    let length = (size[0] as usize)
        .checked_mul(4)
        .and_then(|stride| stride.checked_mul(size[1] as usize))
        .filter(|&length| length <= MAX_SURFACE_BYTES)
        .ok_or_else(|| {
            Error::new(
                0,
                format!("渲染面 {}×{} 的像素长度超过上限", size[0], size[1]),
            )
        })?;
    Ok(length)
}
