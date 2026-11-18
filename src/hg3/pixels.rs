use super::{Frame, Input, StandardInfo, parse_frames};
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
    decode_input(Input::Bytes(data), Some(id))
}

/// 校验全部元数据后仅读取选中帧的像素；None 选择最小无符号 ID。
/// 重复 ID 始终取文件中第一帧，缺失 ID 返回 None。
pub fn decode_frame_from(
    source: &dyn crate::io::ReadSource,
    id: Option<i32>,
) -> Result<Option<DecodedFrame>> {
    decode_input(Input::source(source)?, id)
}

fn decode_input(data: Input<'_>, id: Option<i32>) -> Result<Option<DecodedFrame>> {
    let directory = FrameDirectory::read(data)?;
    let frame = match id {
        Some(id) => directory.frame(id),
        None => directory
            .frames
            .iter()
            .min_by_key(|frame| frame.metadata.id),
    };
    frame.map(|frame| frame.decode_input(data)).transpose()
}

pub(crate) struct FrameDirectory {
    pub(crate) frames: Vec<IndexedFrame>,
}

impl FrameDirectory {
    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        Self::read(Input::Bytes(data))
    }

    fn read(data: Input<'_>) -> Result<Self> {
        Ok(Self {
            frames: parse_frames(data, |metadata, payload| IndexedFrame { metadata, payload })?,
        })
    }

    pub(crate) fn frame(&self, id: i32) -> Option<&IndexedFrame> {
        self.frames
            .iter()
            .find(|frame| frame.metadata.id == id as u32)
    }
}

pub(crate) struct IndexedFrame {
    pub(crate) metadata: Frame,
    payload: FramePayload,
}

#[derive(Clone, Copy)]
pub(super) enum FramePayload {
    Missing,
    Native(usize, usize),
    Unsupported(usize, &'static str),
}

impl FramePayload {
    pub(super) fn record(&mut self, name: &[u8], tag: usize, length: usize) {
        if matches!(self, Self::Unsupported(..)) {
            return;
        }
        match name {
            b"img_jpg\0" | b"img_wbp\0" | b"img_jpa\0" => {
                *self = Self::Unsupported(tag, "未支持的 HG-3 图像编码");
            }
            b"img0000\0" => {
                if matches!(self, Self::Missing) {
                    *self = Self::Native(tag + 16, length);
                }
            }
            [b'i', b'm', b'g', digits @ .., 0] if digits.iter().all(u8::is_ascii_digit) => {
                *self = Self::Unsupported(tag, "未支持的 HG-3 图像切片");
            }
            _ => {}
        }
    }
}

impl IndexedFrame {
    /// 解码用于建立目录的同一份字节中的本帧。
    pub(crate) fn decode(&self, data: &[u8]) -> Result<DecodedFrame> {
        self.decode_input(Input::Bytes(data))
    }

    /// 共享列表保留解码失败的帧位置；尚未支持的编码继续报告诊断。
    pub(crate) fn decode_shared(&self, data: &[u8]) -> Result<Option<DecodedFrame>> {
        let unsupported = matches!(self.payload, FramePayload::Unsupported(..))
            || self
                .metadata
                .info
                .is_some_and(|info| info.bit_depth as i16 != 32);
        let decoded = self.decode(data);
        if unsupported {
            decoded.map(Some)
        } else {
            Ok(decoded.ok())
        }
    }

    fn decode_input(&self, data: Input<'_>) -> Result<DecodedFrame> {
        let frame = &self.metadata;
        let info = frame
            .info
            .ok_or_else(|| Error::new(frame.offset, "HG-3 帧缺少标准元数据"))?;
        if info.bit_depth as i16 != 32 {
            return Err(Error::new(frame.offset, "未支持的 HG-3 像素位深"));
        }
        crate::hg_pixels::byte_size(info.size)?;
        let (offset, length) = match self.payload {
            FramePayload::Native(offset, length) => (offset, length),
            FramePayload::Unsupported(offset, message) => return Err(Error::new(offset, message)),
            FramePayload::Missing => {
                return Err(Error::new(frame.offset, "HG-3 帧缺少原生像素数据"));
            }
        };
        let payload = data.bytes(offset, length)?;
        decode_payload(&payload, offset, frame, info)
    }
}

fn decode_payload(
    payload: &[u8],
    offset: usize,
    frame: &Frame,
    info: StandardInfo,
) -> Result<DecodedFrame> {
    let start = read::u32_at(payload, 0)?;
    let rows = read::u32_at(payload, 4)?;
    if start != 0 || rows != info.size[1] {
        return Err(Error::new(offset, "未支持的 HG-3 图像切片"));
    }
    let data_length = read::sized(read::u32_at(payload, 8)?, offset + 8)?;
    let data_size = read::sized(read::u32_at(payload, 12)?, offset + 12)?;
    let command_length = read::sized(read::u32_at(payload, 16)?, offset + 16)?;
    let command_size = read::sized(read::u32_at(payload, 20)?, offset + 20)?;
    let (literals, commands) = crate::hg_pixels::inflate_streams(
        payload,
        24,
        data_length,
        data_size,
        command_length,
        command_size,
    )?;
    let pixels = crate::hg_pixels::decode(&literals, &commands, info.size)?;
    Ok(DecodedFrame {
        id: frame.id,
        info,
        pixels,
        attributes: frame.attributes.clone(),
    })
}
