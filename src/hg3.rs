use crate::{Error, Result, read};
use std::collections::BTreeMap;

mod input;
mod pixels;
use input::Input;
pub use pixels::{DecodedFrame, decode_frame, decode_frame_from};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardInfo {
    pub size: [u32; 2],
    pub bit_depth: u32,
    pub offset: [i32; 2],
    pub canvas_size: [u32; 2],
    /// stdinfo 的 Alpha 标记；图像装载时零表示不透明，非零表示使用 Alpha。
    pub reserved: u32,
    pub base: [i32; 2],
}

impl StandardInfo {
    fn parse(payload: &[u8]) -> Result<Self> {
        let word = |offset| read::u32_at(payload, offset);
        Ok(Self {
            size: [word(0)?, word(4)?],
            bit_depth: word(8)?,
            offset: [word(12)? as i32, word(16)? as i32],
            canvas_size: [word(20)?, word(24)?],
            reserved: word(28)?,
            base: [word(32)? as i32, word(36)? as i32],
        })
    }
}

/// 查询一个帧的标准字段；允许后续帧和像素正文位于读取前缀之外。
pub(crate) fn frame_info(data: &[u8], id: u32) -> Result<Option<StandardInfo>> {
    if read::u32_at(data, 4)? != 12 || read::u32_at(data, 8)? != 0x300 {
        return Err(Error::new(4, "未支持的 HG-3 头或版本"));
    }
    let mut frame = 12;
    loop {
        let next = read::sized(read::u32_at(data, frame)?, frame)?;
        if next != 0 && next < 8 {
            return Err(Error::new(frame, "HG-3 帧偏移与帧头重叠"));
        }
        if read::u32_at(data, frame + 4)? == id {
            let end = if next == 0 {
                data.len()
            } else {
                (frame + next).min(data.len())
            };
            let data = &data[..end];
            let mut tag = frame + 8;
            loop {
                let header = read::range(data, tag, 16)?;
                let length = read::u32_at(header, 12)?;
                if &header[..8] == b"stdinfo\0" {
                    if length < 40 {
                        return Err(Error::new(tag, "HG-3 标准字段长度不足"));
                    }
                    return StandardInfo::parse(read::range(data, tag + 16, 40)?).map(Some);
                }
                let next = read::sized(read::u32_at(header, 8)?, tag + 8)?;
                if next == 0 {
                    return Ok(None);
                }
                if next < 16 || (next - 16) < length as usize {
                    return Err(Error::new(tag + 8, "HG-3 标签偏移与当前标签重叠"));
                }
                tag += next;
            }
        }
        if next == 0 {
            return Ok(None);
        }
        frame += next;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub offset: usize,
    pub id: u32,
    pub info: Option<StandardInfo>,
    pub attributes: BTreeMap<u16, [u32; 5]>,
}

/// HG-3 0x300 的帧目录、标准元数据与属性记录，不解码像素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub frames: Vec<Frame>,
}

impl Metadata {
    pub fn parse(data: &[u8]) -> Result<Self> {
        Self::parse_input(Input::Bytes(data))
    }

    /// 从随机读取源读取全部帧与标签元数据，跳过像素正文。
    pub fn read(source: &dyn crate::io::ReadSource) -> Result<Self> {
        Self::parse_input(Input::source(source)?)
    }

    fn parse_input(data: Input<'_>) -> Result<Self> {
        if data.len() < 4 || data.len() > read::MAX_BYTES || data.array::<4>(0)? != *b"HG-3" {
            return Err(Error::new(0, "无效 HG-3 容器"));
        }
        if data.u32_at(4)? != 12 || data.u32_at(8)? != 0x300 {
            return Err(Error::new(4, "未支持的 HG-3 头或版本"));
        }
        let mut frames = Vec::new();
        let mut position = 12;
        let mut tags = 0;
        loop {
            if frames.len() == read::MAX_ITEMS {
                return Err(Error::new(position, "HG-3 帧数超过读取上限"));
            }
            let next = read::sized(data.u32_at(position)?, position)?;
            let end = if next == 0 {
                data.len()
            } else {
                if next < 8 {
                    return Err(Error::new(position, "HG-3 帧偏移与帧头重叠"));
                }
                data.check(position, next)?;
                position + next
            };
            let mut frame = Frame {
                offset: position,
                id: data.u32_at(position + 4)?,
                info: None,
                attributes: BTreeMap::new(),
            };
            let mut tag = position + 8;
            loop {
                if tags == read::MAX_ITEMS {
                    return Err(Error::new(tag, "HG-3 标签数超过读取上限"));
                }
                tags += 1;
                let bounded = data.bounded(end);
                let header = bounded.array::<16>(tag)?;
                let next_tag = read::sized(read::u32_at(&header, 8)?, tag + 8)?;
                let length = read::sized(read::u32_at(&header, 12)?, tag + 12)?;
                bounded.check(tag + 16, length)?;
                if header[..8] == *b"stdinfo\0" && frame.info.is_none() {
                    if length < 40 {
                        return Err(Error::new(0, "输入范围越界"));
                    }
                    frame.info = Some(StandardInfo::parse(&bounded.array::<40>(tag + 16)?)?);
                }
                if header[..3] == *b"ats" {
                    // 编号可越过名称末尾；已校验的 next_tag 高字节会终止前缀。
                    if let Some((_, id)) = crate::text::integer_prefix(&header[3..12]) {
                        if length < 20 {
                            return Err(Error::new(0, "输入范围越界"));
                        }
                        let bytes = bounded.array::<20>(tag + 16)?;
                        frame.attributes.insert(
                            id as u16,
                            std::array::from_fn(|i| {
                                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                            }),
                        );
                    }
                }
                if next_tag == 0 {
                    break;
                }
                if next_tag < 16 + length {
                    return Err(Error::new(tag + 8, "HG-3 标签偏移与当前标签重叠"));
                }
                bounded.check(tag, next_tag)?;
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
