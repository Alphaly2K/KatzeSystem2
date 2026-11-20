use super::{Rect, Surface};

impl Surface {
    /// 从底部起算 Y 坐标，适配原图像对象的矩形填充坐标。
    pub fn fill_rect_from_bottom(&mut self, rect: Rect, argb: u32) -> bool {
        let Some(rect) = rect.intersect(self.bounds()) else {
            return false;
        };
        self.fill_rect(
            Rect {
                top: self.size()[1] as i32 - rect.bottom,
                bottom: self.size()[1] as i32 - rect.top,
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
            self.pixels_mut()[start..start + count]
                .copy_from_slice(&source.pixels()[from..from + count]);
        }
        true
    }

    /// 使用基线普通 32 位 Alpha 规则绘制，并向全零的左右邻接像素延展边缘 RGB。
    pub fn blend_from(&mut self, source: &Self, rect: Rect, at: [i32; 2], opacity: u8) -> bool {
        self.blend_from_bounds(source, rect, at, opacity).is_some()
    }

    pub(crate) fn blend_from_bounds(
        &mut self,
        source: &Self,
        rect: Rect,
        at: [i32; 2],
        opacity: u8,
    ) -> Option<Rect> {
        let region = self.clip_blend(source, rect, at)?;
        let bounds = Rect::from_xywh(
            region.x as i32,
            region.y as i32,
            region.width as i32,
            region.height as i32,
        );
        self.blend_region(source, region, opacity);
        Some(bounds)
    }

    pub(super) fn blend_region(&mut self, source: &Self, region: ImageRegion, opacity: u8) {
        self.blend_region_raw(source, region, u32::from(opacity));
    }

    pub(super) fn blend_region_raw(&mut self, source: &Self, region: ImageRegion, opacity: u32) {
        let count = region.width * 4;
        let stride = self.stride();
        let source_stride = source.stride();
        for row in 0..region.height {
            let start = (region.y + row) * stride + region.x * 4;
            let from = (region.source_y + row) * source_stride + region.source_x * 4;
            let input = &source.pixels()[from..from + count];
            if region.x > 0 {
                extend_color(&mut self.pixels_mut()[start - 4..start], &input[..4]);
            }
            let target = &mut self.pixels_mut()[start..start + count];
            if opacity & 0xffff == 255 && count >= 64 {
                blend_full_opacity_row(target, input);
            } else {
                blend_pixels(target, input, opacity);
            }
            if region.x + region.width < self.size()[0] as usize {
                extend_color(
                    &mut self.pixels_mut()[start + count..start + count + 4],
                    &input[count - 4..],
                );
            }
        }
    }

    pub(super) fn blend_region_within(&mut self, region: ImageRegion, opacity: u8) {
        let count = region.width * 4;
        let stride = self.stride();
        // 原图像按底端起始的地址向前逐行、逐像素读写，重叠区域会读取先前写入的结果。
        for row in (0..region.height).rev() {
            let start = (region.y + row) * stride + region.x * 4;
            let from = (region.source_y + row) * stride + region.source_x * 4;
            if region.x > 0 {
                let pixel: [u8; 4] = self.pixels()[from..from + 4].try_into().unwrap();
                extend_color(&mut self.pixels_mut()[start - 4..start], &pixel);
            }
            for column in 0..region.width {
                let source = from + column * 4;
                let target = start + column * 4;
                let pixel: [u8; 4] = self.pixels()[source..source + 4].try_into().unwrap();
                blend_pixel(
                    &mut self.pixels_mut()[target..target + 4],
                    &pixel,
                    u32::from(opacity),
                );
            }
            if region.x + region.width < self.size()[0] as usize {
                let pixel: [u8; 4] = self.pixels()[from + count - 4..from + count]
                    .try_into()
                    .unwrap();
                extend_color(
                    &mut self.pixels_mut()[start + count..start + count + 4],
                    &pixel,
                );
            }
        }
    }

    pub(super) fn clip_blend(
        &self,
        source: &Self,
        rect: Rect,
        at: [i32; 2],
    ) -> Option<ImageRegion> {
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
        let right = (x + i64::from(clipped.right - clipped.left)).min(i64::from(self.size()[0]));
        let bottom = (y + i64::from(clipped.bottom - clipped.top)).min(i64::from(self.size()[1]));
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

pub(super) struct ImageRegion {
    pub(super) x: usize,
    pub(super) y: usize,
    pub(super) source_x: usize,
    pub(super) source_y: usize,
    pub(super) width: usize,
    pub(super) height: usize,
}

fn extend_color(target: &mut [u8], source: &[u8]) {
    if target == [0; 4] {
        target[..3].copy_from_slice(&source[..3]);
    }
}

fn blend_pixels(target: &mut [u8], source: &[u8], opacity: u32) {
    for (target, source) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        blend_pixel(target, source, opacity);
    }
}

fn blend_full_opacity_row(target: &mut [u8], source: &[u8]) {
    let mut target = target.chunks_exact_mut(64);
    let mut source = source.chunks_exact(64);
    for (target, source) in target.by_ref().zip(source.by_ref()) {
        // 零像素块包含颜色和 Alpha；带隐藏颜色的透明像素仍参与普通混合。
        if source == [0; 64] {
            continue;
        }
        if target == [0; 64] {
            target.copy_from_slice(source);
        } else {
            blend_pixels(target, source, 255);
        }
    }
    blend_pixels(target.into_remainder(), source.remainder(), 255);
}

pub(super) fn blend_pixel(target: &mut [u8], source: &[u8], opacity: u32) {
    // 原混合入口按透明度低十六位生成字节查找表，随后才查源 Alpha。
    let alpha = (((opacity & 0xffff) * (u32::from(source[3]) + 1)) >> 8) & 0xff;
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
