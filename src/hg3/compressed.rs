use crate::{Error, Result, read};
use std::io::Cursor;

pub(super) fn webp(payload: &[u8], offset: usize, size: [u32; 2]) -> Result<Vec<u8>> {
    let error = |err| Error::new(offset, format!("HG-3 WebP 解码失败：{err}"));
    let mut reader = image_webp::WebPDecoder::new(Cursor::new(payload)).map_err(error)?;
    if reader.dimensions() != (size[0], size[1]) {
        return Err(Error::new(offset, "HG-3 WebP 与标准字段的尺寸不一致"));
    }
    if reader.is_animated() {
        return Err(Error::new(offset, "未支持的 HG-3 WebP 动画"));
    }
    reader.set_memory_limit(read::MAX_BYTES);
    let len = reader
        .output_buffer_size()
        .filter(|&len| len <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(offset, "HG-3 WebP 输出超过读取上限"))?;
    let mut pixels = vec![0; len];
    reader.read_image(&mut pixels).map_err(error)?;
    if !reader.has_alpha() {
        pixels = pixels
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect();
    }
    Ok(pixels)
}

pub(super) fn apply_alpha(payload: &[u8], pixels: &mut [u8]) -> Option<()> {
    let packed = read::u32_at(payload, 0).ok()? as usize;
    let unpacked = read::sized(read::u32_at(payload, 4).ok()?, 4).ok()?;
    if unpacked < pixels.len() / 4 {
        return None;
    }
    let alpha = read::inflate(read::range(payload, 8, packed).ok()?, unpacked).ok()?;
    for (pixel, alpha) in pixels.chunks_exact_mut(4).zip(alpha) {
        pixel[3] = alpha;
    }
    Some(())
}
