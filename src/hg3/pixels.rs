use super::{Metadata, StandardInfo};
use crate::{Error, Result, read};

/// 从上到下、紧密排列的 BGRA8 像素；Alpha 保持独立通道。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub id: u32,
    pub info: StandardInfo,
    pub pixels: Vec<u8>,
    pub attributes: std::collections::BTreeMap<u16, [u32; 5]>,
}

/// 解码首个匹配 ID 的原生 32 位完整帧；缺失 ID 返回 None。
pub fn decode_frame(data: &[u8], id: i32) -> Result<Option<DecodedFrame>> {
    let metadata = Metadata::parse(data)?;
    let Some(frame) = metadata.frame(id) else {
        return Ok(None);
    };
    let info = frame
        .info
        .ok_or_else(|| Error::new(frame.offset, "HG-3 帧缺少标准元数据"))?;
    if info.bit_depth as i16 != 32 {
        return Err(Error::new(frame.offset, "未支持的 HG-3 像素位深"));
    }
    crate::hg_pixels::byte_size(info.size)?;
    let next = read::u32_at(data, frame.offset)? as usize;
    let end = if next == 0 {
        data.len()
    } else {
        frame.offset + next
    };
    let mut position = frame.offset + 8;
    let mut image = None;
    loop {
        let header = read::range(&data[..end], position, 16)?;
        let name = &header[..8];
        let next = read::u32_at(header, 8)? as usize;
        let length = read::u32_at(header, 12)? as usize;
        if matches!(name, b"img_jpg\0" | b"img_wbp\0" | b"img_jpa\0") {
            return Err(Error::new(position, "未支持的 HG-3 图像编码"));
        }
        if &name[..3] == b"img" && name[3..7].iter().all(u8::is_ascii_digit) && name[7] == 0 {
            if name != b"img0000\0" {
                return Err(Error::new(position, "未支持的 HG-3 图像切片"));
            }
            if image.is_none() {
                image = Some((position + 16, length));
            }
        }
        if next == 0 {
            break;
        }
        position += next;
    }
    let (offset, length) =
        image.ok_or_else(|| Error::new(frame.offset, "HG-3 帧缺少原生像素数据"))?;
    let payload = read::range(data, offset, length)?;
    let start = read::u32_at(payload, 0)?;
    let rows = read::u32_at(payload, 4)?;
    if start != 0 || rows != info.size[1] {
        return Err(Error::new(offset, "未支持的 HG-3 图像切片"));
    }
    let data_length = read::sized(read::u32_at(payload, 8)?, offset + 8)?;
    let data_size = read::sized(read::u32_at(payload, 12)?, offset + 12)?;
    let command_length = read::sized(read::u32_at(payload, 16)?, offset + 16)?;
    let command_size = read::sized(read::u32_at(payload, 20)?, offset + 20)?;
    let literals = read::inflate(read::range(payload, 24, data_length)?, data_size)?;
    let commands = read::inflate(
        read::range(payload, 24 + data_length, command_length)?,
        command_size,
    )?;
    let pixels = crate::hg_pixels::decode(&literals, &commands, info.size)?;
    Ok(Some(DecodedFrame {
        id: frame.id,
        info,
        pixels,
        attributes: frame.attributes.clone(),
    }))
}
