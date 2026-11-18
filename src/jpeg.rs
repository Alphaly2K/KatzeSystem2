use crate::hg3::{DecodedFrame, StandardInfo};
use crate::{Error, Result, read};
use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Read;

pub(crate) const SIGNATURE: [u8; 2] = [0xff, 0xd8];

pub(crate) fn decode(
    input: impl Read,
    offset: usize,
    expected: Option<[u32; 2]>,
) -> Result<([u32; 2], Vec<u8>)> {
    let error = |err| Error::new(offset, format!("JPEG 解码失败：{err}"));
    let mut reader = Decoder::new(input);
    reader.set_max_decoding_buffer_size(read::MAX_BYTES);
    reader.read_info().map_err(error)?;
    let info = reader
        .info()
        .ok_or_else(|| Error::new(offset, "JPEG 缺少像素尺寸"))?;
    let size = [u32::from(info.width), u32::from(info.height)];
    if expected.is_some_and(|expected| expected != size) {
        return Err(Error::new(offset, "HG-3 JPEG 与标准字段的尺寸不一致"));
    }
    if size.contains(&0) || u64::from(size[0]) * u64::from(size[1]) * 4 > read::MAX_BYTES as u64 {
        return Err(Error::new(offset, "JPEG 像素尺寸超出范围"));
    }
    if !matches!(info.pixel_format, PixelFormat::L8 | PixelFormat::RGB24) {
        return Err(Error::new(offset, "未支持的 JPEG 色彩格式"));
    }
    let decoded = reader.decode().map_err(error)?;
    let pixels = match info.pixel_format {
        PixelFormat::L8 => decoded.into_iter().flat_map(|c| [c, c, c, 255]).collect(),
        _ => decoded
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
    };
    Ok((size, pixels))
}

pub(crate) fn decode_frame(input: impl Read, bit_depth: u32) -> Result<DecodedFrame> {
    let (size, mut pixels) = decode(input, 0, None)?;
    // 独立 JPEG 沿用原位图的颜色字节和底端起始存储。
    reverse_rows(&mut pixels, size[0] as usize);
    Ok(DecodedFrame {
        id: 0,
        info: StandardInfo {
            size,
            bit_depth,
            offset: [0; 2],
            canvas_size: size,
            reserved: 0,
            base: [0; 2],
        },
        pixels,
        attributes: Default::default(),
    })
}

pub(crate) fn reverse_rows(pixels: &mut [u8], width: usize) {
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
