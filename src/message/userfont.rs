use super::{GlyphMask, GlyphPaint, TextEncoding};
use crate::{
    Error, Result,
    graphics::{Image, Rect, Surface},
};
use std::{collections::BTreeMap, rc::Rc};

#[derive(Default)]
pub(super) struct UserFonts {
    source: Option<Rc<[u8]>>,
    mappings: BTreeMap<u16, u32>,
    images: BTreeMap<u16, Option<Rc<Image>>>,
}

impl UserFonts {
    pub fn set_source(&mut self, bytes: Option<&[u8]>) {
        self.source = bytes.map(Rc::from);
    }

    pub fn insert(&mut self, character: u16, frame: u32) -> Result<()> {
        self.mappings.insert(character, frame);
        if let Some(bytes) = &self.source {
            let image = crate::hg3::decode_frame(bytes, frame as i32)?
                .map(Image::from_frame)
                .transpose()?
                .map(Rc::new);
            self.images.insert(character, image);
        }
        Ok(())
    }

    pub fn reconfigure(&mut self) -> Result<()> {
        let mappings = self.mappings.clone();
        self.images.clear();
        for (character, frame) in mappings {
            self.insert(character, frame)?;
        }
        Ok(())
    }

    pub fn glyph(&self, character: char, encoding: TextEncoding) -> Option<Glyph> {
        let key = match encoding {
            TextEncoding::Unicode => u16::try_from(u32::from(character)).ok()?,
            TextEncoding::Cp932 => {
                let mut text = [0; 4];
                let (bytes, _, invalid) =
                    encoding_rs::SHIFT_JIS.encode(character.encode_utf8(&mut text));
                if invalid {
                    return None;
                }
                match bytes.as_ref() {
                    [a] => u16::from(*a),
                    [a, b] => u16::from_be_bytes([*a, *b]),
                    _ => return None,
                }
            }
        };
        self.images
            .get(&key)
            .map(|image| Glyph::Image(image.clone()))
    }
}

pub(super) enum Glyph {
    Mask(GlyphMask),
    Image(Option<Rc<Image>>),
}

impl Glyph {
    pub fn paint(&self, target: &mut Surface, paint: &GlyphPaint) -> Result<Rect> {
        match self {
            Self::Mask(mask) if mask.coverage().is_empty() => Ok(Rect::default()),
            Self::Mask(mask) => mask.paint(target, paint),
            Self::Image(None) => Ok(Rect::default()),
            Self::Image(Some(image)) => paint_image(target, image, paint),
        }
    }
}

fn paint_image(target: &mut Surface, image: &Image, paint: &GlyphPaint) -> Result<Rect> {
    let size = paint.font_size;
    if size == 0 {
        return Err(Error::new(0, "用户字形不支持零字号"));
    }
    let dimensions = std::array::from_fn(|i| {
        let canvas = image.info().canvas_size[i];
        if canvas == 0 {
            image.info().size[i]
        } else {
            canvas
        }
    });
    if dimensions
        .iter()
        .any(|&n| u64::from(n) > u64::from(size) * 4)
    {
        return Err(Error::new(0, "用户字形的大比例缩小尚未接入"));
    }
    let extent = paint
        .effects
        .shadow_offset
        .map(|n| size.wrapping_add(n as u32).wrapping_add(2));
    if extent[1] > extent[0] || extent.iter().any(|&n| n > i32::MAX as u32) {
        return Err(Error::new(0, "用户字形效果范围超出基线临时面"));
    }
    let mut source = Surface::new([extent[0]; 2])?;
    let output = source.size();
    for y in 0..size.min(output[1].saturating_sub(1)) {
        for x in 0..size.min(output[0].saturating_sub(1)) {
            if let Some(pixel) = sample(image, dimensions, size, [x, y]) {
                let at = ((y + 1) as usize * output[0] as usize + (x + 1) as usize) * 4;
                source.pixels_mut()[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
    let bounds = Rect::from_xywh(0, 0, extent[0] as i32, extent[1] as i32);
    let mut canvas = Surface::new(source.size())?;
    if paint
        .effects
        .apply(&mut canvas, &source, size, paint.shadow_color)?
    {
        canvas.blend_from(&source, bounds, [0; 2], 255);
        target.copy_from(&canvas, bounds, [0; 2]);
    } else {
        target.copy_from(&source, bounds, [0; 2]);
    }
    Ok(bounds)
}

fn sample(image: &Image, dimensions: [u32; 2], size: u32, at: [u32; 2]) -> Option<[u8; 4]> {
    let [width, height] = image.info().size;
    let fixed: [u64; 2] = std::array::from_fn(|i| {
        (u64::from(dimensions[i]) * 32768 / u64::from(size)) * u64::from(at[i])
    });
    let [x, y] = fixed.map(|n| (n >> 15) as u32);
    if x >= width || y >= height {
        return None;
    }
    let pixel = |x, y| {
        let index = (y as usize * width as usize + x as usize) * 4;
        <[u8; 4]>::try_from(&image.surface().pixels()[index..index + 4]).unwrap()
    };
    let first = pixel(x, y);
    if dimensions == [size; 2] {
        return Some(first);
    }
    let right = if x + 1 < width {
        pixel(x + 1, y)
    } else {
        first
    };
    let below = if y + 1 < height {
        pixel(x, y + 1)
    } else {
        first
    };
    // 基线在任一边缘把对角样本回退到当前像素。
    let diagonal = if x + 1 < width && y + 1 < height {
        pixel(x + 1, y + 1)
    } else {
        first
    };
    if [first, right, below, diagonal].iter().all(|p| p[3] == 0) {
        return None;
    }
    let [fx, fy] = fixed.map(|n| (n & 32767) as u32);
    let weights = [
        ((32768 - fx) * (32768 - fy)) >> 15,
        (fx * (32768 - fy)) >> 15,
        ((32768 - fx) * fy) >> 15,
        (fx * fy) >> 15,
    ];
    Some(std::array::from_fn(|channel| {
        if channel == 3 && [first, right, below, diagonal].iter().all(|p| p[3] == 255) {
            return 255;
        }
        ([first, right, below, diagonal]
            .into_iter()
            .zip(weights)
            .map(|(pixel, weight)| u32::from(pixel[channel]) * weight)
            .sum::<u32>()
            >> 15) as u8
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_glyphs_keep_edge_interpolation_and_ignore_foreground_color_and_weight() {
        let mut image = Image::new([2, 3]).unwrap();
        for y in 0..3 {
            for x in 0..2 {
                let at = (y * 2 + x) as usize * 4;
                image.pixels_mut()[at..at + 4].copy_from_slice(&[
                    10 + x * 40 + y * 15,
                    20 + x * 25 + y * 30,
                    30 + x * 35 + y * 20,
                    255,
                ]);
            }
        }
        let glyph = Glyph::Image(Some(Rc::new(image)));
        let mut paint = GlyphPaint {
            color: 0xffffff,
            shadow_color: 0,
            font_size: 4,
            double_coverage: false,
            effects: super::super::GlyphEffects::default(),
        };
        let mut first = Surface::new([12; 2]).unwrap();
        assert_eq!(
            glyph.paint(&mut first, &paint).unwrap(),
            Rect::from_xywh(0, 0, 7, 7)
        );
        let pixel = |x: usize, y: usize| {
            let at = (y * 12 + x) * 4;
            &first.pixels()[at..at + 4]
        };
        assert_eq!(pixel(4, 2), [55, 56, 72, 255]);
        assert_eq!(pixel(2, 4), [55, 89, 83, 255]);
        paint.color = 0x123456;
        paint.double_coverage = true;
        let mut second = Surface::new([12; 2]).unwrap();
        glyph.paint(&mut second, &paint).unwrap();
        assert_eq!(first, second);
    }
}
