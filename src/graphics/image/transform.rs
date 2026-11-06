use super::Image;
use crate::Result;
use crate::graphics::Surface;

impl Image {
    /// RGB 依次按无符号百分比调整；DWORD 乘法回绕后除以一百，结果钳制到字节。
    pub fn adjust_rgb_percent(&mut self, rgb: [u32; 3]) {
        if rgb == [100; 3] {
            return;
        }
        for pixel in self.pixels_mut().chunks_exact_mut(4) {
            for (value, percent) in pixel[..3].iter_mut().zip(rgb.into_iter().rev()) {
                *value = (u32::from(*value).wrapping_mul(percent) / 100).min(255) as u8;
            }
        }
        self.filter_revision = self.filter_revision.wrapping_add(1);
    }

    /// 水平或垂直翻转像素，并相对原始画布反射偏移、基点及属性坐标。
    pub fn flip(&mut self, horizontal: bool) {
        let axis = usize::from(!horizontal);
        let size = self.info.size.map(|value| value as usize);
        let stride = self.surface.stride();
        for y in 0..size[1] {
            for x in 0..size[0] {
                if (horizontal && x >= size[0] / 2) || (!horizontal && y >= size[1] / 2) {
                    break;
                }
                let a = y * stride + x * 4;
                let b = if horizontal {
                    y * stride + (size[0] - 1 - x) * 4
                } else {
                    (size[1] - 1 - y) * stride + x * 4
                };
                for channel in 0..4 {
                    self.surface.pixels_mut().swap(a + channel, b + channel);
                }
            }
        }
        let canvas = self.info.canvas_size[axis] as i32;
        self.info.offset[axis] = canvas
            .wrapping_sub(self.info.offset[axis])
            .wrapping_sub(self.info.size[axis] as i32);
        self.info.base[axis] = canvas.wrapping_sub(self.info.base[axis]);
        for values in self.attributes.values_mut() {
            values[axis] = (canvas as u32).wrapping_sub(values[axis]);
        }
        self.filter_revision = self.filter_revision.wrapping_add(1);
    }

    /// 在顶端起始坐标中逆时针旋转；仅接受原始的零、九十、一百八十、二百七十度。
    /// 其他角度不修改图像并返回 false，不先对三百六十度取余。
    pub fn rotate_right_angle(&mut self, degrees: i32) -> Result<bool> {
        if degrees == 0 {
            return Ok(true);
        }
        if !matches!(degrees, 90 | 180 | 270) {
            return Ok(false);
        }
        let [width, height] = self.info.size;
        let size = if degrees == 180 {
            [width, height]
        } else {
            [height, width]
        };
        let mut surface = Surface::new(size)?;
        for y in 0..height {
            for x in 0..width {
                let [to_x, to_y] = match degrees {
                    90 => [y, width - 1 - x],
                    180 => [width - 1 - x, height - 1 - y],
                    270 => [height - 1 - y, x],
                    _ => unreachable!(),
                };
                let from = y as usize * self.surface.stride() + x as usize * 4;
                let to = to_y as usize * surface.stride() + to_x as usize * 4;
                surface.pixels_mut()[to..to + 4]
                    .copy_from_slice(&self.surface.pixels()[from..from + 4]);
            }
        }
        let canvas = self.info.canvas_size;
        let rotate = |point: [u32; 2], extent: [u32; 2]| match degrees {
            90 => [
                canvas[1].wrapping_sub(point[1]).wrapping_sub(extent[1]),
                point[0],
            ],
            180 => std::array::from_fn(|axis| {
                canvas[axis]
                    .wrapping_sub(point[axis])
                    .wrapping_sub(extent[axis])
            }),
            270 => [
                point[1],
                canvas[0].wrapping_sub(point[0]).wrapping_sub(extent[0]),
            ],
            _ => unreachable!(),
        };
        self.info.offset = rotate(self.info.offset.map(|value| value as u32), [width, height])
            .map(|value| value as i32);
        self.info.base =
            rotate(self.info.base.map(|value| value as u32), [0; 2]).map(|value| value as i32);
        for values in self.attributes.values_mut() {
            let point = rotate([values[0], values[1]], [0; 2]);
            values[..2].copy_from_slice(&point);
        }
        if degrees != 180 {
            self.info.canvas_size.swap(0, 1);
        }
        self.info.size = size;
        self.surface = surface;
        self.filter_revision = self.filter_revision.wrapping_add(1);
        Ok(true)
    }
}
