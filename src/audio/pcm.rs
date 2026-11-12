use crate::io::{ReadSource, SourceReader};
use crate::{Error, Result, read};
use lewton::inside_ogg::OggStreamReader;

/// 交错排列的有符号 16 位 PCM；一帧包含每个声道的一个采样。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    sample_rate: u32,
    channels: u8,
    samples: std::sync::Arc<[i16]>,
}

impl Pcm {
    pub fn new(sample_rate: u32, channels: u8, samples: Vec<i16>) -> Result<Self> {
        if sample_rate == 0 || !matches!(channels, 1 | 2) {
            return Err(Error::new(0, "PCM 需要非零采样率及单声道或双声道"));
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(Error::new(0, "PCM 数据未对齐完整采样帧"));
        }
        if samples.len() > read::MAX_BYTES / 2 {
            return Err(Error::new(0, "PCM 数据超过读取上限"));
        }
        Ok(Self {
            sample_rate,
            channels,
            samples: samples.into(),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u8 {
        self.channels
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
}

/// 解码单个完整 Ogg Vorbis 流；保留采样率、声道顺序及末页的帧数。
pub fn decode_vorbis(bytes: &[u8]) -> Result<Pcm> {
    decode_vorbis_source(bytes)
}

/// 从独立随机读取源解码完整 Ogg Vorbis 流；不持有资源路由或文件路径。
pub fn decode_vorbis_source<S: ReadSource + ?Sized>(source: &S) -> Result<Pcm> {
    let length = source.len()?;
    if length > read::MAX_BYTES as u64 {
        return Err(Error::new(0, "Ogg Vorbis 数据超过读取上限"));
    }
    let (serial, frames) = stream_end(source, length as usize)?;
    let error = |error| Error::new(0, format!("Ogg Vorbis 解码失败：{error}"));
    let mut decoder = OggStreamReader::new(SourceReader::new(source)).map_err(error)?;
    let rate = decoder.ident_hdr.audio_sample_rate;
    let channels = decoder.ident_hdr.audio_channels;
    if rate == 0 || !matches!(channels, 1 | 2) {
        return Err(Error::new(0, "Ogg Vorbis 需要非零采样率及单声道或双声道"));
    }
    let count = usize::try_from(frames)
        .ok()
        .and_then(|frames| frames.checked_mul(channels as usize))
        .filter(|&count| count <= read::MAX_BYTES / 2)
        .ok_or_else(|| Error::new(0, "Ogg Vorbis 解码采样数量超过读取上限"))?;
    let mut samples = Vec::with_capacity(count);
    while let Some(packet) = decoder.read_dec_packet_itl().map_err(error)? {
        if decoder.stream_serial() != serial {
            return Err(Error::new(0, "Ogg Vorbis 不支持串联或混合逻辑流"));
        }
        if samples
            .len()
            .checked_add(packet.len())
            .is_none_or(|length| length > read::MAX_BYTES / 2)
        {
            return Err(Error::new(0, "Ogg Vorbis 解码采样数量超过读取上限"));
        }
        samples.extend(packet);
    }
    if samples.len() < count {
        return Err(Error::new(0, "Ogg Vorbis 采样不足末页声明的帧数"));
    }
    samples.truncate(count);
    Pcm::new(rate, channels, samples)
}

fn stream_end<S: ReadSource + ?Sized>(source: &S, length: usize) -> Result<(u32, u64)> {
    let mut position = 0;
    let mut serial = None;
    while position < length {
        let mut header = [0; 27];
        read_source(source, length, position, &mut header)?;
        if &header[..4] != b"OggS" || header[4] != 0 {
            return Err(Error::new(position, "Ogg 页签名或版本无效"));
        }
        let current = u32::from_le_bytes(header[14..18].try_into().unwrap());
        if let Some(serial) = serial {
            if current != serial || header[5] & 2 != 0 {
                return Err(Error::new(position, "Ogg Vorbis 不支持串联或混合逻辑流"));
            }
        } else {
            if header[5] & 2 == 0 {
                return Err(Error::new(position, "Ogg 首个页面缺少起始标记"));
            }
            serial = Some(current);
        }
        let mut table = [0; 255];
        let table = &mut table[..header[26] as usize];
        read_source(source, length, position + 27, table)?;
        let body = position + 27 + table.len();
        let body_length: usize = table.iter().map(|&value| value as usize).sum();
        if body.checked_add(body_length).is_none_or(|end| end > length) {
            return Err(Error::new(body, "输入范围越界"));
        }
        position = body + body_length;
        if header[5] & 4 != 0 {
            if position != length {
                return Err(Error::new(position, "Ogg 结束页后仍有数据"));
            }
            return Ok((
                current,
                u64::from_le_bytes(header[6..14].try_into().unwrap()),
            ));
        }
    }
    Err(Error::new(position, "Ogg 流缺少结束页"))
}

fn read_source<S: ReadSource + ?Sized>(
    source: &S,
    length: usize,
    position: usize,
    output: &mut [u8],
) -> Result<()> {
    if position
        .checked_add(output.len())
        .is_none_or(|end| end > length)
    {
        return Err(Error::new(position, "输入范围越界"));
    }
    source.read_exact_at(position as u64, output)
}
