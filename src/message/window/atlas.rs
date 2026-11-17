use crate::graphics::{Image, Rect, Surface, resample};
use crate::message::GlyphPaint;
use crate::{Error, Result};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Atlas {
    source: Option<Image>,
    characters: BTreeMap<u16, Rect>,
}

impl Atlas {
    pub fn set_source(&mut self, image: &Image) -> Result<()> {
        let info = image.info();
        let canvas = std::array::from_fn(|axis| {
            if info.canvas_size[axis] == 0 {
                info.size[axis]
            } else {
                info.canvas_size[axis]
            }
        });
        let mut snapshot = Image::new(canvas)?;
        let [width, height] = info.size.map(|value| value as i32);
        snapshot.copy_scaled_from(
            image,
            Rect::from_xywh(info.offset[0], info.offset[1], width, height),
            Rect::from_xywh(0, 0, width, height),
        )?;
        self.source = Some(snapshot);
        Ok(())
    }

    pub fn insert(&mut self, character: u16, area: Rect) {
        self.characters.entry(character).or_insert(area);
    }

    pub fn contains(&self, character: u16) -> bool {
        self.characters.contains_key(&character)
    }

    pub fn render(&self, character: u16, paint: &GlyphPaint) -> Result<Option<Surface>> {
        let (Some(source), Some(&area)) = (&self.source, self.characters.get(&character)) else {
            return Ok(None);
        };
        let size = paint.font_size;
        let extent = size
            .checked_add(4)
            .filter(|&value| value <= i32::MAX as u32)
            .ok_or_else(|| Error::new(0, "文本窗口用户字形尺寸溢出"))?;
        let mut foreground = Surface::new([extent; 2])?;
        resample::copy_scaled(
            &mut foreground,
            Some(source.surface()),
            area,
            Rect::from_xywh(1, 1, size as i32, size as i32),
        )?;
        let mut output = Surface::new(foreground.size())?;
        // 旧字形缓存按底端行序执行偏移阴影。
        if paint.effects.shadow {
            resample::flip_rows(&mut foreground);
        }
        if paint
            .effects
            .apply(&mut output, &foreground, size, paint.shadow_color)?
        {
            if paint.effects.shadow {
                resample::flip_rows(&mut foreground);
                resample::flip_rows(&mut output);
            }
            output.blend_from(&foreground, foreground.bounds(), [0; 2], 255);
        } else {
            output.copy_from(&foreground, foreground.bounds(), [0; 2]);
        }
        Ok(Some(output))
    }
}
