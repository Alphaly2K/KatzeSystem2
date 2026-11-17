use super::{Image, Images};
use crate::Result;
use crate::graphics::Rect;

impl Image {
    fn composite_region(&mut self, source: Option<&Self>, position: [i32; 2], clip: Rect) {
        let input = source.unwrap_or(self);
        let bounds = Rect::from_xywh(
            position[0],
            position[1],
            input.info.size[0] as i32,
            input.info.size[1] as i32,
        );
        let Some(clipped) = bounds.intersect(clip) else {
            return;
        };
        let source_rect = Rect::from_xywh(
            clipped.left.wrapping_sub(position[0]),
            clipped.top.wrapping_sub(position[1]),
            clipped.right.wrapping_sub(clipped.left),
            clipped.bottom.wrapping_sub(clipped.top),
        );
        let Some(mut region) =
            self.surface
                .clip_blend(&input.surface, source_rect, [clipped.left, clipped.top])
        else {
            return;
        };
        region.y = self.info.size[1] as usize - region.y - region.height;
        region.source_y = input.info.size[1] as usize - region.source_y - region.height;
        if input.is_rgb() {
            self.blend_rgb_region(source, region);
        } else if let Some(source) = source {
            self.surface.blend_region(&source.surface, region, 0);
        } else {
            self.surface.blend_region_within(region, 0);
        }
    }
}

impl Images {
    pub(crate) fn composite_region(
        &mut self,
        target: u32,
        source: u32,
        position: [i32; 2],
        clip: Rect,
    ) -> Result<()> {
        let source_data = self.entry_mut(source)?.data;
        let target_data = self.entry_mut(target)?.data;
        self.pixel_image(source_data)?;
        self.pixel_image(target_data)?;
        if source_data == target_data {
            self.get_mut(target)?.composite_region(None, position, clip);
        } else {
            let source = self.data.remove(&source_data).unwrap();
            let result = self.get_mut(target).map(|target| {
                target.composite_region(Some(&source.image), position, clip);
            });
            self.data.insert(source_data, source);
            result?;
        }
        Ok(())
    }
}
