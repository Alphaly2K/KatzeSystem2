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
                    let payload = &bounded.array::<40>(tag + 16)?;
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
