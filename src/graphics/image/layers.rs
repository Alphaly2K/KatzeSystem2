use super::Image;
use crate::Result;
use crate::graphics::Rect;

impl Image {
    /// 合成静态图层：裁剪边界取并集，画布逐轴取最大，各层对齐首层基点，属性由后层覆盖。
    /// 空图层列表或全部裁剪矩形为空时返回 None；不改变输入图像。
    pub fn from_layers(layers: &[Self]) -> Result<Option<Self>> {
        let mut bounds: Option<Rect> = None;
        let mut canvas = [0i32; 2];
        for layer in layers {
            let info = layer.info();
            let rect = Rect::from_xywh(
                info.offset[0],
                info.offset[1],
                info.size[0] as i32,
                info.size[1] as i32,
            );
            if rect.left < rect.right && rect.top < rect.bottom {
                bounds = Some(bounds.map_or(rect, |bounds| Rect {
                    left: bounds.left.min(rect.left),
                    top: bounds.top.min(rect.top),
                    right: bounds.right.max(rect.right),
                    bottom: bounds.bottom.max(rect.bottom),
                }));
            }
            for (axis, maximum) in canvas.iter_mut().enumerate() {
                let size = if info.canvas_size[axis] == 0 {
                    info.size[axis]
                } else {
                    info.canvas_size[axis]
                };
                *maximum = (*maximum).max(size as i32);
            }
        }
        let Some(bounds) = bounds else {
            return Ok(None);
        };
        let mut image = Self::new([
            bounds.right.wrapping_sub(bounds.left) as u32,
            bounds.bottom.wrapping_sub(bounds.top) as u32,
        ])?;
        image.info.offset = [bounds.left, bounds.top];
        image.info.canvas_size = canvas.map(|value| value as u32);
        image.info.base = layers[0].info.base;
        for layer in layers {
            let at = [
                layer.info.offset[0]
                    .wrapping_sub(bounds.left)
                    .wrapping_add(image.info.base[0].wrapping_sub(layer.info.base[0])),
                layer.info.offset[1]
                    .wrapping_sub(bounds.top)
                    .wrapping_add(image.info.base[1].wrapping_sub(layer.info.base[1])),
            ];
            if let Some(mut region) =
                image
                    .surface
                    .clip_blend(&layer.surface, layer.surface.bounds(), at)
            {
                region.y = image.info.size[1] as usize - region.y - region.height;
                region.source_y = layer.info.size[1] as usize - region.source_y - region.height;
                image.surface.blend_region(&layer.surface, region, 255);
            }
            image.attributes.extend(&layer.attributes);
        }
        Ok(Some(image))
    }
}
