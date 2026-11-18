use super::{Image, Rect, resource::ResourceGeometry};
use crate::Result;

impl Image {
    pub(crate) fn compose_bitmap_layers(layers: &[Self]) -> Result<Option<Self>> {
        let Some(geometry) = ResourceGeometry::from_layers(layers) else {
            return Ok(None);
        };
        if geometry.size.contains(&0) {
            return Ok(None);
        }
        let mut image = Self::new(geometry.size)?;
        geometry.apply(&mut image);
        let bounds = geometry.bounds([0.; 2], [0.; 2]);
        for layer in layers {
            let info = layer.info();
            let extent = Rect::from_xywh(
                info.offset[0],
                info.offset[1],
                info.size[0] as i32,
                info.size[1] as i32,
            );
            let Some(clip) = bounds.intersect(extent) else {
                continue;
            };
            // 先按原始裁剪位置求交，再以基点差移动目标矩形。
            let at = [clip.left, clip.top];
            let at = std::array::from_fn(|axis| {
                at[axis]
                    .wrapping_sub(geometry.offset[axis])
                    .wrapping_add(geometry.base[axis].wrapping_sub(info.base[axis]))
            });
            if let Some(mut region) =
                image
                    .surface
                    .clip_blend(&layer.surface, layer.surface.bounds(), at)
            {
                region.y = geometry.size[1] as usize - region.y - region.height;
                region.source_y = info.size[1] as usize - region.source_y - region.height;
                image.surface.blend_region(&layer.surface, region, 255);
            }
            image.attributes.extend(&layer.attributes);
        }
        Ok(Some(image))
    }

    /// 合成静态图层：裁剪边界取并集，画布逐轴取最大，保留首层基点和后层属性。
    /// 空图层列表或全部裁剪矩形为空时返回 None；不改变输入图像。
    pub fn from_layers(layers: &[Self]) -> Result<Option<Self>> {
        let Some(geometry) = ResourceGeometry::from_layers(layers) else {
            return Ok(None);
        };
        if geometry.size.contains(&0) {
            return Ok(None);
        }
        let mut image = Self::new(geometry.size)?;
        let clip = image.surface.bounds();
        for layer in layers {
            let properties = geometry.layer_properties(layer);
            image.draw_list_from(
                layer,
                properties.position,
                properties.base_point,
                255,
                clip,
                &mut Some(layer.info.reserved != 0),
            )?;
            image.attributes.extend(&layer.attributes);
        }
        geometry.apply(&mut image);
        Ok(Some(image))
    }
}
