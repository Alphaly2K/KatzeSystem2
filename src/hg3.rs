use crate::{Error, Result, read};

mod pixels;
pub use pixels::{DecodedFrame, decode_frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardInfo {
    pub size: [u32; 2],
    pub bit_depth: u32,
    pub offset: [i32; 2],
    pub canvas_size: [u32; 2],
    pub reserved: u32,
    pub base: [i32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub offset: usize,
    pub id: u32,
    pub info: Option<StandardInfo>,
}

/// HG-3 0x300 的帧目录和标准元数据，不解码像素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub frames: Vec<Frame>,
}

impl Metadata {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.get(..4) != Some(b"HG-3") || data.len() > read::MAX_BYTES {
            return Err(Error::new(0, "无效 HG-3 容器"));
        }
        if read::u32_at(data, 4)? != 12 || read::u32_at(data, 8)? != 0x300 {
            return Err(Error::new(4, "未支持的 HG-3 头或版本"));
        }
        let mut frames = Vec::new();
        let mut position = 12;
        let mut tags = 0;
        loop {
            if frames.len() == read::MAX_ITEMS {
                return Err(Error::new(position, "HG-3 帧数超过读取上限"));
            }
            let next = read::sized(read::u32_at(data, position)?, position)?;
            let end = if next == 0 {
                data.len()
            } else {
                if next < 8 {
                    return Err(Error::new(position, "HG-3 帧偏移与帧头重叠"));
                }
                read::range(data, position, next)?;
                position + next
            };
            let mut frame = Frame {
                offset: position,
                id: read::u32_at(data, position + 4)?,
                info: None,
            };
            let mut tag = position + 8;
            loop {
                if tags == read::MAX_ITEMS {
                    return Err(Error::new(tag, "HG-3 标签数超过读取上限"));
                }
                tags += 1;
                let header = read::range(&data[..end], tag, 16)?;
                let next_tag = read::sized(read::u32_at(header, 8)?, tag + 8)?;
                let length = read::sized(read::u32_at(header, 12)?, tag + 12)?;
                let payload = read::range(&data[..end], tag + 16, length)?;
                if header[..8] == *b"stdinfo\0" && frame.info.is_none() {
                    read::range(payload, 0, 40)?;
                    frame.info = Some(StandardInfo {
                        size: [read::u32_at(payload, 0)?, read::u32_at(payload, 4)?],
                        bit_depth: read::u32_at(payload, 8)?,
                        offset: [
                            read::u32_at(payload, 12)? as i32,
                            read::u32_at(payload, 16)? as i32,
                        ],
                        canvas_size: [read::u32_at(payload, 20)?, read::u32_at(payload, 24)?],
                        reserved: read::u32_at(payload, 28)?,
                        base: [
                            read::u32_at(payload, 32)? as i32,
                            read::u32_at(payload, 36)? as i32,
                        ],
                    });
                }
                if next_tag == 0 {
                    break;
                }
                if next_tag < 16 + length {
                    return Err(Error::new(tag + 8, "HG-3 标签偏移与当前标签重叠"));
                }
                read::range(&data[..end], tag, next_tag)?;
                tag += next_tag;
            }
            frames.push(frame);
            if next == 0 {
                break;
            }
            position = end;
        }
        Ok(Self { frames })
    }

    /// 按文件顺序取第一个匹配 ID，不把 ID 当作数组下标。
    pub fn frame(&self, id: i32) -> Option<&Frame> {
        self.frames.iter().find(|frame| frame.id == id as u32)
    }
}
