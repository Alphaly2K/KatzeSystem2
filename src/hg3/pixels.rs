use super::{Frame, Input, StandardInfo, compressed, parse_frames};
use crate::{Error, Result, hg_pixels, jpeg, read};
use std::ops::Range;

/// 从上到下、紧密排列的 BGRA8 像素；Alpha 保持独立通道。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub id: u32,
    /// 图像对象的几何与位深；HG-3 解码后的位深固定为 32。
    pub info: StandardInfo,
    pub pixels: Vec<u8>,
    pub attributes: std::collections::BTreeMap<u16, [u32; 5]>,
}

/// 解码首个匹配 ID 的 24／32 位帧；缺失 ID 返回 None。
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
            frames: parse_frames(data, |metadata, mut payload| {
                payload.rest.sort_unstable();
                IndexedFrame { metadata, payload }
            })?,
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

#[derive(Default)]
pub(super) struct FramePayload {
    first: Option<Payload>,
    rest: Vec<(u16, usize, usize)>,
    jpeg: Option<Payload>,
    webp: Option<Payload>,
    alpha: Option<Payload>,
    mode: Option<Payload>,
    jpa: Option<usize>,
}

type Payload = (usize, usize);

impl FramePayload {
    pub(super) fn record(&mut self, name: &[u8], tag: usize, length: usize) {
        let range = (tag + 16, length);
        let target = match name {
            b"img_jpg\0" => &mut self.jpeg,
            b"img_wbp\0" => &mut self.webp,
            [b'i', b'm', b'g', b'_', b'a', b'l', 0, _] => &mut self.alpha,
            b"imgmode\0" => {
                self.mode = Some(range);
                return;
            }
            b"img_jpa\0" => {
                self.jpa.get_or_insert(tag);
                return;
            }
            b"img0000\0" => &mut self.first,
            [b'i', b'm', b'g', digits @ .., 0] if digits.iter().all(u8::is_ascii_digit) => {
                let id = digits
                    .iter()
                    .fold(0, |id, &byte| id * 10 + u16::from(byte - b'0'));
                self.rest.push((id, tag + 16, length));
                return;
            }
            _ => return,
        };
        target.get_or_insert(range);
    }

    fn unsupported(&self) -> Option<usize> {
        if self.first.is_none() && self.jpeg.is_none() && self.webp.is_none() {
            self.jpa
        } else {
            None
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
        let unsupported = self.payload.unsupported().is_some()
            || self
                .metadata
                .info
                .is_some_and(|info| !matches!(info.bit_depth as i16, 24 | 32));
        let decoded = self.decode(data);
        if unsupported {
            decoded.map(Some)
        } else {
            Ok(decoded.ok())
        }
    }

    fn decode_input(&self, data: Input<'_>) -> Result<DecodedFrame> {
        let frame = &self.metadata;
        let mut info = frame
            .info
            .ok_or_else(|| Error::new(frame.offset, "HG-3 帧缺少标准元数据"))?;
        let channels = match info.bit_depth as i16 {
            24 => 3,
            32 => 4,
            _ => return Err(Error::new(frame.offset, "未支持的 HG-3 像素位深")),
        };
        let size = hg_pixels::byte_size(info.size)?;
        if let Some(tag) = self.payload.unsupported() {
            return Err(Error::new(tag, "未支持的 HG-3 图像编码"));
        }
        let width = info.size[0] as usize;
        let mut pixels = if let Some((offset, len)) = self.payload.webp {
            let mut pixels = compressed::webp(&data.bytes(offset, len)?, offset, info.size)?;
            if channels == 3 {
                // 原入口按三字节布局复制 RGBA 解码缓冲的前缀。
                pixels = pixels[..size / 4 * 3]
                    .chunks_exact(3)
                    .flat_map(|c| [c[0], c[1], c[2], 255])
                    .collect();
            }
            jpeg::reverse_rows(&mut pixels, width);
            pixels
        } else if let Some((offset, len)) = self.payload.jpeg {
            let (_, mut pixels) =
                jpeg::decode(data.bytes(offset, len)?.as_ref(), offset, Some(info.size))?;
            if channels == 4 {
                if let Some((at, length)) = self.payload.alpha {
                    compressed::apply_alpha(&data.bytes(at, length)?, &mut pixels);
                }
            }
            jpeg::reverse_rows(&mut pixels, width);
            pixels
        } else {
            self.decode_slices(data, info.size, size, channels)?
        };
        let swap = if let Some((offset, len)) = self.payload.mode {
            if len < 4 {
                return Err(Error::new(offset, "HG-3 图像模式字段过短"));
            }
            data.u32_at(offset)? == 1
        } else {
            self.payload.jpeg.is_some() || self.payload.jpa.is_some()
        };
        if swap {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        info.bit_depth = 32;
        Ok(DecodedFrame {
            id: frame.id,
            info,
            pixels,
            attributes: frame.attributes.clone(),
        })
    }

    fn decode_slices(
        &self,
        data: Input<'_>,
        dimensions: [u32; 2],
        size: usize,
        channels: usize,
    ) -> Result<Vec<u8>> {
        let frame = &self.metadata;
        let first = self
            .payload
            .first
            .ok_or_else(|| Error::new(frame.offset, "HG-3 帧缺少原生像素数据"))?;
        let stride = dimensions[0] as usize * 4;
        let height = dimensions[1] as usize;
        let decode = |(offset, length)| {
            decode_block(&data.bytes(offset, length)?, offset, dimensions, channels)
        };
        let (rows, block) = decode(first)?;
        let (mut pixels, mut covered) = if rows.len() == height {
            (block, None)
        } else {
            let mut pixels = vec![0; size];
            pixels[rows.start * stride..rows.end * stride].copy_from_slice(&block);
            let mut covered = vec![false; height];
            covered[rows].fill(true);
            (pixels, Some(covered))
        };

        // 按连续编号查找；同编号取文件中最早出现的标签。
        let mut next = 1;
        for &(id, offset, length) in &self.payload.rest {
            if id < next {
                continue;
            }
            if id != next {
                break;
            }
            let (rows, block) = decode((offset, length))?;
            pixels[rows.start * stride..rows.end * stride].copy_from_slice(&block);
            if let Some(covered) = &mut covered {
                covered[rows].fill(true);
            }
            next += 1;
        }
        if covered.is_some_and(|rows| rows.contains(&false)) {
            return Err(Error::new(frame.offset, "HG-3 原生像素数据未覆盖全部行"));
        }
        Ok(pixels)
    }
}

fn decode_block(
    payload: &[u8],
    offset: usize,
    size: [u32; 2],
    channels: usize,
) -> Result<(Range<usize>, Vec<u8>)> {
    let start = read::u32_at(payload, 0)?;
    let rows = read::u32_at(payload, 4)?;
    let end = start
        .checked_add(rows)
        .filter(|&end| rows != 0 && end <= size[1])
        .ok_or_else(|| Error::new(offset, "HG-3 图像切片行范围越界"))?;
    let data_length = read::sized(read::u32_at(payload, 8)?, offset + 8)?;
    let data_size = read::sized(read::u32_at(payload, 12)?, offset + 12)?;
    let command_length = read::sized(read::u32_at(payload, 16)?, offset + 16)?;
    let command_size = read::sized(read::u32_at(payload, 20)?, offset + 20)?;
    let (literals, commands) = hg_pixels::inflate_streams(
        payload,
        24,
        data_length,
        data_size,
        command_length,
        command_size,
    )?;
    let mut pixels = hg_pixels::decode_channels(&literals, &commands, [size[0], rows], channels)?;
    if channels == 3 {
        pixels = pixels
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect();
    }
    Ok((start as usize..end as usize, pixels))
}
