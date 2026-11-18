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
    frame_metadata(data, version, frame).map(Some)
}

/// 遍历 0x25 的标准字段，不读取像素数据。
pub(crate) fn visit_bounds_frames(
    data: &[u8],
    mut visit: impl FnMut(u32, StandardInfo),
) -> Result<bool> {
    let version = read::u32_at(data, 8)? as u16;
    if matches!(version, 0x10 | 0x20) {
        return Ok(false);
    }
    if version != 0x25 || read::u32_at(data, 4)? != 12 {
        return Err(Error::new(4, "未支持的 HG-2 头或版本"));
    }
    let mut frame = 12;
    for _ in 0..read::MAX_ITEMS {
        let id = read::u32_at(data, frame + 0x28)?;
        visit(id, frame_metadata(data, version, frame)?);
        let next = read::sized(read::u32_at(data, frame + 0x40)?, frame + 0x40)?;
        if next == 0 {
            return Ok(true);
        }
        if next < 0x4c {
            return Err(Error::new(frame + 0x40, "HG-2 帧偏移与帧头重叠"));
        }
        read::range(data, frame, next)?;
        frame += next;
    }
    Err(Error::new(frame, "HG-2 帧数超过读取上限"))
}

fn frame_metadata(data: &[u8], version: u16, frame: usize) -> Result<StandardInfo> {
    let word = |offset| read::u32_at(data, frame + offset);
    let size = [word(0)?, word(4)?];
    Ok(StandardInfo {
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
    })
}

/// 解码 HG-2 文件中的第一帧，输出顶端起始 BGRA8。
pub fn decode_frame(data: &[u8]) -> Result<DecodedFrame> {
    let frame = find_frame(data, None)?.ok_or_else(|| Error::new(12, "HG-2 缺少首帧"))?;
    frame.decode()
}

/// 解码第一个匹配 ID 的 HG-2 帧。0x10 只有一帧，忽略所请求的 ID。
pub fn decode_frame_id(data: &[u8], id: u32) -> Result<Option<DecodedFrame>> {
    find_frame(data, Some(id))?
        .map(Hg2Frame::decode)
        .transpose()
}

pub(crate) fn decode_shared_frame(data: &[u8]) -> Result<Option<DecodedFrame>> {
    let Some(frame) = find_frame(data, Some(0))? else {
        return Ok(None);
    };
    if !matches!(frame.info.bit_depth, 24 | 32) {
        return Err(Error::new(frame.offset + 8, "未支持的 HG-2 位深"));
    }
    Ok(frame.decode().ok())
}

struct Hg2Frame<'a> {
    bytes: &'a [u8],
    offset: usize,
    id: u32,
    info: StandardInfo,
    payload: usize,
    significant: u32,
}

fn find_frame(data: &[u8], id: Option<u32>) -> Result<Option<Hg2Frame<'_>>> {
    if data.get(..4) != Some(b"HG-2") || data.len() > read::MAX_BYTES {
        return Err(Error::new(0, "无效 HG-2 容器"));
    }
    let version = read::u32_at(data, 8)? as u16;
    if read::u32_at(data, 4)? != 12 || !matches!(version, 0x10 | 0x20 | 0x25) {
        return Err(Error::new(4, "未支持的 HG-2 头或版本"));
    }
    let header = match version {
        0x10 => 36,
        0x20 => 68,
        _ => 76,
    };
    let mut offset = 12;
    for _ in 0..read::MAX_ITEMS {
        let word = |at| read::u32_at(data, offset + at);
        read::range(data, offset, header)?;
        let frame_id = if version == 0x10 { 0 } else { word(0x28)? };
        let next = if version == 0x10 {
            0
        } else {
            read::sized(word(0x40)?, offset + 0x40)?
        };
        if next != 0 && next < header {
            return Err(Error::new(offset + 0x40, "HG-2 帧偏移与帧头重叠"));
        }
        if version == 0x10 || id.is_none_or(|id| id == frame_id) {
            let payload = if version == 0x10 {
                header
            } else {
                let start = 0x24 + read::sized(word(0x24)?, offset + 0x24)?;
                if start < header {
                    return Err(Error::new(offset + 0x24, "HG-2 像素偏移与帧头重叠"));
                }
                start
            };
            let bytes = if next == 0 {
                &data[offset..]
            } else {
                read::range(data, offset, next)?
            };
            let size = [word(0)?, word(4)?];
            let depth = word(8)?;
            let info = StandardInfo {
                size,
                bit_depth: depth & 0xffff,
                canvas_size: if version == 0x10 {
                    size
                } else {
                    [word(0x2c)?, word(0x30)?]
                },
                offset: if version == 0x10 {
                    [0; 2]
                } else {
                    [word(0x34)? as i32, word(0x38)? as i32]
                },
                reserved: if version == 0x10 { 0 } else { word(0x3c)? },
                base: if version == 0x25 {
                    [word(0x44)? as i32, word(0x48)? as i32]
                } else {
                    [0; 2]
                },
            };
            return Ok(Some(Hg2Frame {
                bytes,
                offset,
                id: frame_id,
                info,
                payload,
                significant: depth >> 16,
            }));
        }
        if next == 0 {
            return Ok(None);
        }
        offset += next;
    }
    Err(Error::new(offset, "HG-2 帧数超过读取上限"))
}

impl Hg2Frame<'_> {
    fn decode(self) -> Result<DecodedFrame> {
        let info = self.info;
        if !matches!(info.bit_depth, 24 | 32) {
            return Err(Error::new(self.offset + 8, "未支持的 HG-2 位深"));
        }
        hg_pixels::byte_size(info.size)?;
        let stream_size = |at| read::sized(read::u32_at(self.bytes, at)?, self.offset + at);
        let (literals, commands) = hg_pixels::inflate_streams(
            self.bytes,
            self.payload,
            stream_size(20)?,
            stream_size(24)?,
            stream_size(28)?,
            stream_size(32)?,
        )?;
        let channels = (info.bit_depth / 8) as usize;
        let mut pixels = hg_pixels::decode_channels(&literals, &commands, info.size, channels)?;
        if self.significant != 0 {
            expand_significant(
                &mut pixels,
                info.size[0] as usize * channels,
                self.significant,
            );
        }
        if channels == 3 {
            pixels = pixels
                .chunks_exact(3)
                .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
                .collect();
        }
        Ok(DecodedFrame {
            id: self.id,
            info,
            pixels,
            attributes: Default::default(),
        })
    }
}

fn expand_significant(pixels: &mut [u8], stride: usize, significant: u32) {
    let left = 8u32.wrapping_sub(significant);
    let right = significant.wrapping_sub(left);
    let mask = (255u32.wrapping_shl(left) & 255) * 0x0101_0101;
    for row in pixels.chunks_exact_mut(stride) {
        let mut groups = row.chunks_exact_mut(4);
        for group in groups.by_ref() {
            let value = u32::from_le_bytes(group.try_into().unwrap());
            let expanded = (value.wrapping_shl(left) & mask) | (value.wrapping_shr(right) & !mask);
            group.copy_from_slice(&expanded.to_le_bytes());
        }
        for value in groups.into_remainder() {
            let byte = u32::from(*value);
            *value = (byte.wrapping_shl(left) | byte.wrapping_shr(right)) as u8;
        }
    }
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
