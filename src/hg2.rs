use crate::hg3::{DecodedFrame, StandardInfo};
use crate::{Error, Result, hg_pixels, read};
use std::io::Write;

pub(crate) fn frame_info(data: &[u8], id: u32) -> Result<Option<StandardInfo>> {
    let version = read::u32_at(data, 8)? as u16;
    if !matches!(version, 0x10 | 0x20 | 0x25) {
        return Ok(None);
    }
    let mut frame = 12;
    if version != 0x10 {
        while read::u32_at(data, frame + 0x28)? != id {
            let next = read::sized(read::u32_at(data, frame + 0x40)?, frame + 0x40)?;
            if next == 0 {
                return Ok(None);
            }
            if next < if version == 0x25 { 0x4c } else { 0x44 } {
                return Err(Error::new(frame + 0x40, "HG-2 帧偏移与帧头重叠"));
            }
            frame += next;
        }
    }
    let word = |offset| read::u32_at(data, frame + offset);
    let size = [word(0)?, word(4)?];
    Ok(Some(StandardInfo {
        size,
        bit_depth: word(8)? >> 16,
        offset: if version == 0x10 {
            [0, 0]
        } else {
            [word(0x34)? as i32, word(0x38)? as i32]
        },
        canvas_size: if version == 0x10 {
            size
        } else {
            [word(0x2c)?, word(0x30)?]
        },
        reserved: 0,
        base: if version == 0x25 {
            [word(0x44)? as i32, word(0x48)? as i32]
        } else {
            [0, 0]
        },
    }))
}

/// 解码 HG-2 0x25 的单帧 24／32 位图像，输出顶端起始 BGRA8。
pub fn decode_frame(data: &[u8]) -> Result<DecodedFrame> {
    if data.get(..4) != Some(b"HG-2") || data.len() > read::MAX_BYTES {
        return Err(Error::new(0, "无效 HG-2 容器"));
    }
    read::range(data, 0, 88)?;
    if read::u32_at(data, 4)? != 12 || read::u32_at(data, 8)? != 0x25 {
        return Err(Error::new(4, "未支持的 HG-2 头或版本"));
    }
    let bit_depth = read::u32_at(data, 20)?;
    if !matches!(bit_depth, 24 | 32) || read::u32_at(data, 76)? != 0 {
        return Err(Error::new(20, "未支持的 HG-2 位深或多帧图像"));
    }
    let info = StandardInfo {
        size: [read::u32_at(data, 12)?, read::u32_at(data, 16)?],
        bit_depth,
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
    let pixels = if bit_depth == 24 {
        hg_pixels::decode_channels(&literals, &commands, info.size, 3)?
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect()
    } else {
        hg_pixels::decode(&literals, &commands, info.size)?
    };
    Ok(DecodedFrame {
        id: read::u32_at(data, 52)?,
        info,
        pixels,
        attributes: Default::default(),
    })
}

/// 编码完整 BGRA8 图像；裁去透明边界，画布保持原尺寸，基点及帧号为零。
pub fn encode_frame(size: [u32; 2], pixels: &[u8]) -> Result<Vec<u8>> {
    encode_frame_with_depth(size, pixels, 32)
}

/// 编码顶端起始 BGRA8 视图；24 位忽略 Alpha 且保留完整尺寸，32 位裁去透明边界。
pub fn encode_frame_with_depth(size: [u32; 2], pixels: &[u8], bit_depth: u32) -> Result<Vec<u8>> {
    if !matches!(bit_depth, 24 | 32) {
        return Err(Error::new(0, "HG-2 编码需要 24 或 32 位像素"));
    }
    if pixels.len() != hg_pixels::byte_size(size)? {
        return Err(Error::new(0, "HG-2 像素长度与尺寸不符"));
    }
    let [width, height] = size.map(|n| n as usize);
    let mut bounds = [width, height, 0, 0];
    let mut alpha = false;
    for (i, pixel) in pixels.chunks_exact(4).enumerate() {
        alpha |= bit_depth == 32 && pixel[3] != 255;
        if bit_depth == 24 || pixel[3] != 0 {
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
            if bit_depth == 24 {
                cropped.extend_from_slice(&pixel[..3]);
            } else {
                cropped.extend_from_slice(if pixel[3] == 0 { &[0; 4] } else { pixel });
            }
        }
    }
    let (literals, commands) = if bit_depth == 24 {
        hg_pixels::encode_channels(&cropped, cropped_size, 3)?
    } else {
        hg_pixels::encode(&cropped, cropped_size)?
    };
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
        bit_depth,
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
