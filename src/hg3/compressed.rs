use crate::{Error, Result, read};
use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Cursor;

pub(super) fn jpeg(payload: &[u8], offset: usize, size: [u32; 2]) -> Result<Vec<u8>> {
    let error = |err| Error::new(offset, format!("HG-3 JPEG 解码失败：{err}"));
    let mut reader = Decoder::new(payload);
    reader.set_max_decoding_buffer_size(read::MAX_BYTES);
    reader.read_info().map_err(error)?;
    let info = reader.info().unwrap();
    if [u32::from(info.width), u32::from(info.height)] != size {
        return Err(Error::new(offset, "HG-3 JPEG 与标准字段的尺寸不一致"));
    }
    if !matches!(info.pixel_format, PixelFormat::L8 | PixelFormat::RGB24) {
        return Err(Error::new(offset, "未支持的 HG-3 JPEG 色彩格式"));
    }
    let decoded = reader.decode().map_err(error)?;
    Ok(match info.pixel_format {
        PixelFormat::L8 => decoded.into_iter().flat_map(|c| [c, c, c, 255]).collect(),
        _ => decoded
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
    })
}

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

pub(super) fn reverse_rows(pixels: &mut [u8], width: usize) {
    let stride = width * 4;
    let half = pixels.len() / stride / 2;
    let (first, rest) = pixels.split_at_mut(half * stride);
    for (a, b) in first
        .chunks_exact_mut(stride)
        .zip(rest.rchunks_exact_mut(stride))
    {
        a.swap_with_slice(b);
    }
}
