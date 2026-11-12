use crate::hg3::{DecodedFrame, StandardInfo};
use crate::{Error, Result, hg_pixels, read};
use std::io::Write;

/// 解码 HG-2 0x25 的单帧 32 位图像，输出顶端起始 BGRA8。
pub fn decode_frame(data: &[u8]) -> Result<DecodedFrame> {
    if data.get(..4) != Some(b"HG-2") || data.len() > read::MAX_BYTES {
        return Err(Error::new(0, "无效 HG-2 容器"));
    }
    read::range(data, 0, 88)?;
    if read::u32_at(data, 4)? != 12 || read::u32_at(data, 8)? != 0x25 {
        return Err(Error::new(4, "未支持的 HG-2 头或版本"));
    }
    if read::u32_at(data, 20)? != 32 || read::u32_at(data, 76)? != 0 {
        return Err(Error::new(20, "未支持的 HG-2 位深或多帧图像"));
    }
    let info = StandardInfo {
        size: [read::u32_at(data, 12)?, read::u32_at(data, 16)?],
        bit_depth: 32,
        canvas_size: [read::u32_at(data, 56)?, read::u32_at(data, 60)?],
        offset: [
            read::u32_at(data, 64)? as i32,
            read::u32_at(data, 68)? as i32,
        ],
        reserved: read::u32_at(data, 72)?,
        base: [
            read::u32_at(data, 80)? as i32,
            read::u32_at(data, 84)? as i32,
        ],
    };
    hg_pixels::byte_size(info.size)?;
    let data_length = read::sized(read::u32_at(data, 32)?, 32)?;
    let data_size = read::sized(read::u32_at(data, 36)?, 36)?;
    let command_length = read::sized(read::u32_at(data, 40)?, 40)?;
    let command_size = read::sized(read::u32_at(data, 44)?, 44)?;
    let literals = read::inflate(read::range(data, 88, data_length)?, data_size)?;
    let commands = read::inflate(
        read::range(data, 88 + data_length, command_length)?,
        command_size,
    )?;
    Ok(DecodedFrame {
        id: read::u32_at(data, 52)?,
        info,
        pixels: hg_pixels::decode(&literals, &commands, info.size)?,
        attributes: Default::default(),
    })
}

/// 编码完整 BGRA8 图像；裁去透明边界，画布保持原尺寸，基点及帧号为零。
pub fn encode_frame(size: [u32; 2], pixels: &[u8]) -> Result<Vec<u8>> {
    if pixels.len() != hg_pixels::byte_size(size)? {
        return Err(Error::new(0, "HG-2 像素长度与尺寸不符"));
    }
    let [width, height] = size.map(|n| n as usize);
    let mut bounds = [width, height, 0, 0];
    let mut alpha = false;
    for (i, pixel) in pixels.chunks_exact(4).enumerate() {
        alpha |= pixel[3] != 255;
        if pixel[3] != 0 {
            bounds[0] = bounds[0].min(i % width);
            bounds[1] = bounds[1].min(i / width);
            bounds[2] = bounds[2].max(i % width + 1);
            bounds[3] = bounds[3].max(i / width + 1);
        }
    }
    if bounds[2] == 0 {
        bounds = [0, 0, width.min(2), height.min(2)];
    }
    let [left, top, right, bottom] = bounds;
    let cropped_size = [(right - left) as u32, (bottom - top) as u32];
    let mut cropped = Vec::with_capacity(hg_pixels::byte_size(cropped_size)?);
    for y in top..bottom {
        for pixel in pixels[(y * width + left) * 4..(y * width + right) * 4].chunks_exact(4) {
            cropped.extend_from_slice(if pixel[3] == 0 { &[0; 4] } else { pixel });
        }
    }
    let (literals, commands) = hg_pixels::encode(&cropped, cropped_size)?;
    let compress = |bytes: &[u8]| -> Result<Vec<u8>> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder
            .write_all(bytes)
            .map_err(|e| Error::new(0, format!("HG-2 压缩失败：{e}")))?;
        encoder
            .finish()
            .map_err(|e| Error::new(0, format!("HG-2 压缩失败：{e}")))
    };
    let data = compress(&literals)?;
    let control = compress(&commands)?;
    let mut out = b"HG-2".to_vec();
    for value in [
        12,
        0x25,
        cropped_size[0],
        cropped_size[1],
        32,
        0,
        0,
        data.len() as u32,
        literals.len() as u32,
        control.len() as u32,
        commands.len() as u32,
        40,
        0,
        size[0],
        size[1],
        left as u32,
        top as u32,
        u32::from(alpha),
        0,
        0,
        0,
    ] {
        out.extend(value.to_le_bytes());
    }
    out.extend(data);
    out.extend(control);
    Ok(out)
}
