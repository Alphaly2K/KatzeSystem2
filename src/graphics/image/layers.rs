use super::{Image, resource::ResourceGeometry};
use crate::Result;

impl Image {
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
