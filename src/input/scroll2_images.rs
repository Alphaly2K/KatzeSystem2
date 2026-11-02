use crate::{
    Error, Result,
    graphics::{Image, Rect},
};
use std::collections::BTreeMap;

pub(super) fn compose(
    frames: &BTreeMap<u32, Image>,
    extent: i32,
    minimum: i32,
    vertical: bool,
) -> Result<[Image; 4]> {
    let axis = usize::from(vertical);
    let mut result = Vec::with_capacity(4);
    for state in 0..4 {
        let frame = state * 100;
        let cross = (0..3)
            .filter_map(|part| frames.get(&(frame + part)))
            .map(|image| image.info().size[1 - axis])
            .max()
            .unwrap_or(0);
        let size = if vertical {
            [cross, extent as u32]
        } else {
            [extent as u32, cross]
        };
        let mut target = Image::new(size)?;
        let mut offset = 0i32;
        for part in [1, 0, 2] {
            let Some(source) = frames.get(&(frame + part)) else {
                continue;
            };
            let mut length = source.info().size.map(|value| value as i32);
            if part == 0 {
                length[axis] = extent.wrapping_sub(minimum);
            }
            if length.iter().all(|&value| value > 0) {
                copy_scaled(
                    &mut target,
                    source,
                    length.map(|value| value as u32),
                    axis,
                    offset,
                )?;
            }
            offset = offset.wrapping_add(length[axis]);
        }
        if let Some(source) = frames.get(&(frame + 3)) {
            let mut at = [0; 2];
            at[axis] = ((size[axis] >> 1) as i32)
                .wrapping_sub((source.info().size[axis] >> 1) as i32)
                .wrapping_sub(source.info().offset[axis]);
            target.blend_from(source, source.surface().bounds(), at);
        }
        result.push(target);
    }
    Ok(result.try_into().unwrap())
}

fn copy_scaled(
    target: &mut Image,
    source: &Image,
    size: [u32; 2],
    axis: usize,
    offset: i32,
) -> Result<()> {
    let source_size = source.info().size;
    if source_size.contains(&0) {
        return Ok(());
    }
    let mut at = [0; 2];
    at[axis] = offset;
    let Some(bounds) = Rect::from_xywh(at[0], at[1], size[0] as i32, size[1] as i32)
        .intersect(target.surface().bounds())
    else {
        return Ok(());
    };
    let identical = source_size == size;
    let average =
        source_size[0] as u64 > 4 * size[0] as u64 || source_size[1] as u64 > 4 * size[1] as u64;
    let step = if identical || average {
        [0; 2]
    } else {
        let mut step = [0; 2];
        for i in 0..2 {
            step[i] = (source_size[i] as i32)
                .wrapping_shl(15)
                .checked_div(size[i] as i32)
                .filter(|&value| value >= 0)
                .ok_or_else(|| Error::new(0, "滚动条插值步长超出坐标范围"))?
                as u32;
        }
        step
    };
    let target_size = target.info().size;
    let pixels = target.pixels_mut();
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let point = [x.wrapping_sub(at[0]) as u32, y.wrapping_sub(at[1]) as u32];
            let pixel = if identical {
                Some(sample(source, point[0], point[1]))
            } else if average {
                Some(box_sample(source, size, point))
            } else {
                bilinear_sample(source, step, point)?
            };
            if let Some(pixel) = pixel {
                let target = ((target_size[1] as usize - 1 - y as usize) * target_size[0] as usize
                    + x as usize)
                    * 4;
                pixels[target..target + 4].copy_from_slice(&pixel);
            }
        }
    }
    Ok(())
}

fn sample(source: &Image, x: u32, y: u32) -> [u8; 4] {
    let [width, height] = source.info().size;
    let x = x.min(width - 1);
    let y = y.min(height - 1);
    let at = ((height - 1 - y) as usize * width as usize + x as usize) * 4;
    source.surface().pixels()[at..at + 4].try_into().unwrap()
}

fn bilinear_sample(source: &Image, step: [u32; 2], point: [u32; 2]) -> Result<Option<[u8; 4]>> {
    let mut position = [0; 2];
    for i in 0..2 {
        position[i] = point[i]
            .checked_mul(step[i])
            .filter(|&value| value <= i32::MAX as u32)
            .ok_or_else(|| Error::new(0, "滚动条插值坐标超出范围"))?;
    }
    let [x, y] = position.map(|value| value >> 15);
    let samples = [
        sample(source, x, y),
        sample(source, x + 1, y),
        sample(source, x, y + 1),
        sample(source, x + 1, y + 1),
    ];
    if samples.iter().all(|pixel| pixel[3] == 0) {
        return Ok(None);
    }
    let [fx, fy] = position.map(|value| value & 0x7fff);
    let weights = [
        ((0x8000 - fx) * (0x8000 - fy)) >> 15,
        (fx * (0x8000 - fy)) >> 15,
        ((0x8000 - fx) * fy) >> 15,
        (fx * fy) >> 15,
    ];
    Ok(Some(std::array::from_fn(|channel| {
        (samples
            .iter()
            .zip(weights)
            .map(|(pixel, weight)| u32::from(pixel[channel]) * weight)
            .sum::<u32>()
            >> 15) as u8
    })))
}

fn box_sample(source: &Image, size: [u32; 2], point: [u32; 2]) -> [u8; 4] {
    let source_size = source.info().size;
    let scale = std::array::from_fn::<_, 2, _>(|i| source_size[i] as f32 / size[i] as f32);
    let start = std::array::from_fn::<_, 2, _>(|i| (point[i] as f32 * scale[i]) as u32);
    let end = std::array::from_fn::<_, 2, _>(|i| {
        start[i]
            .saturating_add((scale[i] as u32).max(1))
            .min(source_size[i])
    });
    let mut sum = [0u64; 4];
    let mut count = 0;
    for y in start[1]..end[1] {
        for x in start[0]..end[0] {
            for (sum, value) in sum.iter_mut().zip(sample(source, x, y)) {
                *sum += u64::from(value);
            }
            count += 1;
        }
    }
    if count == 0 {
        [0; 4]
    } else {
        sum.map(|sum| (sum / count) as u8)
    }
}
