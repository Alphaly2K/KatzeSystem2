use super::Image;
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
        self.filter_revision = self.filter_revision.wrapping_add(1);
        Ok(true)
    }

    /// 普通 116／735 的图像拷贝：独立裁剪两端，保留来源偏移和底端起算的坐标。
    pub fn copy_scaled_from(&mut self, source: &Self, from: Rect, to: Rect) -> Result<bool> {
        let Some((from, to)) = self.scaled_regions(source, from, to) else {
            return Ok(false);
        };
        resample::copy_scaled(&mut self.surface, Some(&source.surface), from, to)
    }

    /// 对自身执行同一拷贝；等尺寸按行移动，缩放时即时读取此前写入的像素。
    pub fn copy_scaled_within(&mut self, from: Rect, to: Rect) -> Result<bool> {
        let Some((from, to)) = self.scaled_regions(self, from, to) else {
            return Ok(false);
        };
        resample::copy_scaled(&mut self.surface, None, from, to)
    }

    fn scaled_regions(&self, source: &Self, from: Rect, to: Rect) -> Option<(Rect, Rect)> {
        let extent = |rect: Rect| {
            [
                rect.right.wrapping_sub(rect.left),
                rect.bottom.wrapping_sub(rect.top),
            ]
        };
        let equal = extent(from) == extent(to);
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
        let mut from = clip(source, from)?;
        let mut to = clip(self, translate(to, source.info.offset))?;
        if equal {
            let size: [i32; 2] =
                std::array::from_fn(|axis| extent(from)[axis].min(extent(to)[axis]));
            from = Rect::from_xywh(from.left, from.top, size[0], size[1]);
            to = Rect::from_xywh(to.left, to.top, size[0], size[1]);
        }
        Some((from, to))
    }
}

fn low_integer(value: f32) -> u32 {
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&(value as f64)) {
        value as i64 as u32
    } else {
        0
    }
}
