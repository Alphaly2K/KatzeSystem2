use super::{Image, ImageRegion};
use crate::graphics::{Rect, Surface, resample};
use crate::{Error, Result};

impl Image {
    /// 按请求画布与原始画布的比例重采样像素，同步偏移、基点和属性坐标。
    pub fn resize_canvas(&mut self, canvas: [u32; 2]) -> Result<bool> {
        let ratio: [f32; 2] =
            std::array::from_fn(|axis| canvas[axis] as f32 / self.info.canvas_size[axis] as f32);
        let size = std::array::from_fn(|axis| {
            low_integer(self.info.size[axis] as f32 * ratio[axis] + 0.5)
        });
        if self.surface.pixels().is_empty() {
            if size == self.info.size {
                return Ok(false);
            }
            if !(0..2).any(|axis| self.info.size[axis] > size[axis].wrapping_mul(4)) {
                return Err(Error::new(0, "空图像的该缩放路径在原入口中除零"));
            }
        }
        let mut surface = Surface::new(size)?;
        resample::copy_scaled(
            &mut surface,
            Some(&self.surface),
            self.surface.bounds(),
            Rect::from_xywh(0, 0, size[0] as i32, size[1] as i32),
        )?;
        for (axis, ratio) in ratio.into_iter().enumerate() {
            self.info.canvas_size[axis] =
                low_integer(self.info.canvas_size[axis] as f32 * ratio + 0.5);
            self.info.offset[axis] =
                low_integer(self.info.offset[axis] as u32 as f32 * ratio + 0.5) as i32;
            self.info.base[axis] =
                super::list_coordinate(self.info.base[axis] as f32 * ratio + 0.5);
            for values in self.attributes.values_mut() {
                for channel in [axis, axis + 2] {
                    values[channel] =
                        super::list_coordinate(values[channel] as i32 as f32 * ratio) as u32;
                }
            }
        }
        self.info.size = size;
        self.surface = surface;
        self.restore_rgb_alpha();
        self.filter_revision = self.filter_revision.wrapping_add(1);
        Ok(true)
    }

    /// 普通 116／735 的图像拷贝：独立裁剪两端，保留来源偏移和底端起算的坐标。
    pub fn copy_scaled_from(&mut self, source: &Self, from: Rect, to: Rect) -> Result<bool> {
        let Some((from, to)) = self.scaled_regions(source, from, to) else {
            return Ok(false);
        };
        let copied = resample::copy_scaled(&mut self.surface, Some(&source.surface), from, to)?;
        self.restore_rgb_alpha();
        Ok(copied)
    }

    /// 对自身执行同一拷贝；等尺寸按行移动，缩放时即时读取此前写入的像素。
    pub fn copy_scaled_within(&mut self, from: Rect, to: Rect) -> Result<bool> {
        let Some((from, to)) = self.scaled_regions(self, from, to) else {
            return Ok(false);
        };
        let copied = resample::copy_scaled(&mut self.surface, None, from, to)?;
        self.restore_rgb_alpha();
        Ok(copied)
    }

    pub(crate) fn blend_clipped_from(&mut self, source: &Self, from: Rect, to: Rect) -> bool {
        let Some(region) = self.clipped_blend_region(source, from, to) else {
            return false;
        };
        if self.is_rgb() {
            self.blend_rgb_region(Some(source), region);
        } else {
            self.surface.blend_region(&source.surface, region, 255);
        }
        true
    }

    pub(crate) fn blend_clipped_within(&mut self, from: Rect, to: Rect) -> bool {
        let Some(region) = self.clipped_blend_region(self, from, to) else {
            return false;
        };
        if self.is_rgb() {
            self.blend_rgb_region(None, region);
        } else {
            self.surface.blend_region_within(region, 255);
        }
        true
    }

    fn clipped_blend_region(&self, source: &Self, from: Rect, to: Rect) -> Option<ImageRegion> {
        let (from, to) = self.clipped_regions(source, from, to)?;
        // 367 只保留目标裁剪后的起点；混合尺寸来自来源，再由像素边界限制。
        let size = [
            (from.right - from.left).min(self.info.size[0] as i32 - to.left),
            (from.bottom - from.top).min(self.info.size[1] as i32 - to.top),
        ];
        Some(ImageRegion {
            x: to.left as usize,
            y: self.info.size[1] as usize - to.top as usize - size[1] as usize,
            source_x: from.left as usize,
            source_y: source.info.size[1] as usize - from.top as usize - size[1] as usize,
            width: size[0] as usize,
            height: size[1] as usize,
        })
    }

    pub(super) fn blend_rectangles(
        &mut self,
        source: Option<&Self>,
        from: Rect,
        to: Rect,
    ) -> Result<bool> {
        // 目标请求只取起点，合成宽高沿用来源矩形。
        let input = source.unwrap_or(self);
        let Some(region) = self.blend_region(input, from, [to.left, to.top]) else {
            return Ok(false);
        };
        if input.alpha_mask != 0 {
            if self.is_rgb() {
                return Ok(false);
            }
            self.mask_region(source, region)?;
        } else if self.is_rgb() {
            self.blend_rgb_region(source, region);
        } else if let Some(source) = source {
            self.surface.blend_region(&source.surface, region, 255);
        } else {
            self.surface.blend_region_within(region, 255);
        }
        Ok(true)
    }

    fn scaled_regions(&self, source: &Self, from: Rect, to: Rect) -> Option<(Rect, Rect)> {
        let extent = |rect: Rect| {
            [
                rect.right.wrapping_sub(rect.left),
                rect.bottom.wrapping_sub(rect.top),
            ]
        };
        let equal = extent(from) == extent(to);
        let (mut from, mut to) = self.clipped_regions(source, from, to)?;
        if equal {
            let size: [i32; 2] =
                std::array::from_fn(|axis| extent(from)[axis].min(extent(to)[axis]));
            from = Rect::from_xywh(from.left, from.top, size[0], size[1]);
            to = Rect::from_xywh(to.left, to.top, size[0], size[1]);
        }
        Some((from, to))
    }

    fn clipped_regions(&self, source: &Self, from: Rect, to: Rect) -> Option<(Rect, Rect)> {
        let translate = |rect: Rect, offset: [i32; 2]| Rect {
            left: rect.left.wrapping_add(offset[0]),
            right: rect.right.wrapping_add(offset[0]),
            top: rect.top.wrapping_add(offset[1]),
            bottom: rect.bottom.wrapping_add(offset[1]),
        };
        let clip = |image: &Self, rect: Rect| {
            let offset = image.info.offset;
            let bounds = Rect::from_xywh(
                offset[0],
                offset[1],
                image.info.size[0] as i32,
                image.info.size[1] as i32,
            );
            rect.intersect(bounds)
                .map(|rect| translate(rect, offset.map(i32::wrapping_neg)))
        };
        Some((
            clip(source, from)?,
            clip(self, translate(to, source.info.offset))?,
        ))
    }
}

fn low_integer(value: f32) -> u32 {
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&(value as f64)) {
        value as i64 as u32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Images;

    #[test]
    fn masked_blends_keep_channel_rules_and_shared_pixel_feedback() {
        let mut source = Image::new([1, 1]).unwrap();
        source.pixels_mut().copy_from_slice(&[10, 20, 30, 255]);
        source.info.bit_depth = 24;
        let mut target = Image::new([1, 1]).unwrap();
        for (channel, alpha) in [(1, 5), (2, 10), (4, 15), (0x8000_0001, 5), (u32::MAX, 5)] {
            source.set_alpha_mask(channel).unwrap();
            target.pixels_mut().copy_from_slice(&[40, 50, 60, 128]);
            assert!(
                target
                    .blend_rectangles(Some(&source), Rect::from_xywh(0, 0, 1, 1), Rect::default())
                    .unwrap()
            );
            assert_eq!(target.surface.pixels(), [40, 50, 60, alpha]);
        }
        target.info.bit_depth = 24;
        target.pixels_mut().copy_from_slice(&[40, 50, 60, 255]);
        let before = target.clone();
        assert!(
            !target
                .blend_rectangles(Some(&source), Rect::from_xywh(0, 0, 1, 1), Rect::default())
                .unwrap()
        );
        assert_eq!(target, before);
        source.info.bit_depth = 32;
        target.info.bit_depth = 32;
        assert!(
            target
                .blend_rectangles(Some(&source), Rect::from_xywh(0, 0, 1, 1), Rect::default())
                .unwrap_err()
                .message
                .contains("掩码通道")
        );

        let mut images = Images::default();
        let mut image = Image::new([1, 3]).unwrap();
        image
            .pixels_mut()
            .copy_from_slice(&[10, 20, 30, 192, 40, 50, 60, 128, 70, 80, 90, 64]);
        image.set_alpha_mask(4).unwrap();
        let source = images.insert_unowned(image, Some(false)).unwrap();
        let target = images.share(source).unwrap();
        assert!(
            images
                .blend_rectangles(
                    target,
                    source,
                    Rect::from_xywh(0, 0, 1, 2),
                    Rect::from_xywh(0, 1, 0, 0)
                )
                .unwrap()
        );
        assert_eq!(
            images.get(target).unwrap().surface.pixels(),
            [10, 20, 30, 24, 40, 50, 60, 32, 70, 80, 90, 64,]
        );
        assert!(!images.uses_alpha(target).unwrap());
        assert_eq!(images.get(target).unwrap(), images.get(source).unwrap());
    }
}
