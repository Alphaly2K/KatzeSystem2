use super::{Metadata, StandardInfo};
use crate::{Error, Result, read};

/// 从上到下、紧密排列的 BGRA8 像素；Alpha 保持独立通道。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub id: u32,
    pub info: StandardInfo,
    pub pixels: Vec<u8>,
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
    let size = info.size[0]
        .checked_mul(info.size[1])
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| Error::new(frame.offset, "HG-3 像素尺寸溢出"))?;
    let size = read::sized(size, frame.offset)?;
    if info.size.contains(&0) {
        return Err(Error::new(frame.offset, "HG-3 像素尺寸为零"));
    }
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
    let planes = expand_runs(&literals, &commands, size)?;
    let mut pixels = vec![0; size];
    // 位平面按完整游程缓冲分组；小图允许在可见像素之后保留填充。
    let plane_size = planes.len() / 4;
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        for (channel, value) in pixel.iter_mut().enumerate() {
            let mut packed = 0;
            for plane in 0..4 {
                let pair = (planes[plane * plane_size + index] >> (channel * 2)) & 3;
                packed |= pair << (6 - plane * 2);
            }
            *value = (packed >> 1) ^ 0u8.wrapping_sub(packed & 1);
        }
    }
    let stride = info.size[0] as usize * 4;
    for index in 4..stride {
        pixels[index] = pixels[index].wrapping_add(pixels[index - 4]);
    }
    for index in stride..size {
        pixels[index] = pixels[index].wrapping_add(pixels[index - stride]);
    }
    Ok(Some(DecodedFrame {
        id: frame.id,
        info,
        pixels,
    }))
}

struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<bool> {
        let byte = self
            .bytes
            .get(self.position / 8)
            .ok_or_else(|| Error::new(self.position / 8, "HG-3 游程命令截断"))?;
        let value = (byte >> (self.position % 8)) & 1;
        self.position += 1;
        Ok(value != 0)
    }

    fn gamma(&mut self) -> Result<usize> {
        let mut zeros = 0;
        while !self.bit()? {
            zeros += 1;
            if zeros >= 32 {
                return Err(Error::new(self.position / 8, "HG-3 游程长度溢出"));
            }
        }
        let mut value = 1usize;
        for _ in 0..zeros {
            value = (value << 1) | usize::from(self.bit()?);
        }
        Ok(value)
    }
}

fn expand_runs(literals: &[u8], commands: &[u8], minimum_size: usize) -> Result<Vec<u8>> {
    let mut bits = Bits {
        bytes: commands,
        position: 0,
    };
    let mut copy = bits.bit()?;
    let size = bits.gamma()?;
    if size < minimum_size {
        return Err(Error::new(0, "HG-3 游程输出短于像素长度"));
    }
    if size > read::MAX_BYTES {
        return Err(Error::new(0, "HG-3 游程输出超过存储上限"));
    }
    if size % 4 != 0 {
        return Err(Error::new(0, "HG-3 游程输出没有四字节对齐"));
    }
    let mut output = vec![0; size];
    let mut position = 0;
    let mut source = 0;
    while position < size {
        let count = bits.gamma()?;
        let end = position
            .checked_add(count)
            .filter(|&end| end <= size)
            .ok_or_else(|| Error::new(bits.position / 8, "HG-3 游程超出输出范围"))?;
        if copy {
            output[position..end].copy_from_slice(read::range(literals, source, count)?);
            source += count;
        }
        position = end;
        copy = !copy;
    }
    Ok(output)
}
