use crate::io::{ReadSource, SharedSource, SourceReader};
use crate::{Error, Result, read};
use lewton::inside_ogg::OggStreamReader;

/// 单个 Ogg Vorbis 流的逐包解码器；构造读取格式与末页长度，不解码音频包。
/// 返回交错的有符号 16 位采样，最后一包裁到末页声明的帧数。
pub struct VorbisDecoder<'a, S: ReadSource + ?Sized> {
    decoder: OggStreamReader<SourceReader<'a, S>>,
    serial: u32,
    frames: usize,
    decoded: usize,
    finished: bool,
}

impl<'a, S: ReadSource + ?Sized> VorbisDecoder<'a, S> {
    pub fn new(source: &'a S) -> Result<Self> {
        let (serial, frames) = metadata(source)?;
        Self::from_reader(SourceReader::new(source), serial, frames)
    }

    fn from_reader(reader: SourceReader<'a, S>, serial: u32, frames: u64) -> Result<Self> {
        let decoder = OggStreamReader::new(reader).map_err(decode_error)?;
        let rate = decoder.ident_hdr.audio_sample_rate;
        let channels = decoder.ident_hdr.audio_channels;
        if rate == 0 || !matches!(channels, 1 | 2) {
            return Err(Error::new(0, "Ogg Vorbis 需要非零采样率及单声道或双声道"));
        }
        let frames = usize::try_from(frames)
            .ok()
            .filter(|frames| {
                frames
                    .checked_mul(channels as usize)
                    .is_some_and(|count| count <= read::MAX_BYTES / 2)
            })
            .ok_or_else(|| Error::new(0, "Ogg Vorbis 解码采样数量超过读取上限"))?;
        Ok(Self {
            decoder,
            serial,
            frames,
            decoded: 0,
            finished: false,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.decoder.ident_hdr.audio_sample_rate
    }

    pub fn channels(&self) -> u8 {
        self.decoder.ident_hdr.audio_channels
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// 空音频包在内部跳过；读到流末尾返回 None，后续调用继续返回 None。
    /// 解码和后续页面错误在读取对应音频包时报告。
    pub fn read_packet(&mut self) -> Result<Option<Vec<i16>>> {
        if self.finished {
            return Ok(None);
        }
        let expected = self.frames * self.channels() as usize;
        loop {
            let Some(mut packet) = self.decoder.read_dec_packet_itl().map_err(decode_error)? else {
                if self.decoded < expected {
                    return Err(Error::new(0, "Ogg Vorbis 采样不足末页声明的帧数"));
                }
                self.finished = true;
                return Ok(None);
            };
            if self.decoder.stream_serial() != self.serial {
                return Err(Error::new(0, "Ogg Vorbis 不支持串联或混合逻辑流"));
            }
            let count = packet.len();
            let next = self
                .decoded
                .checked_add(count)
                .filter(|&count| count <= read::MAX_BYTES / 2)
                .ok_or_else(|| Error::new(0, "Ogg Vorbis 解码采样数量超过读取上限"))?;
            packet.truncate(expected.saturating_sub(self.decoded));
            self.decoded = next;
            if !packet.is_empty() {
                return Ok(Some(packet));
            }
        }
    }
}

impl VorbisDecoder<'static, dyn ReadSource> {
    /// 持有共享来源，解码状态及其读取游标可以一起移交预取线程。
    pub fn from_shared(source: SharedSource) -> Result<Self> {
        let (serial, frames) = metadata(source.as_ref())?;
        Self::from_reader(SourceReader::from_shared(source), serial, frames)
    }
}

fn decode_error(error: lewton::VorbisError) -> Error {
    Error::new(0, format!("Ogg Vorbis 解码失败：{error}"))
}

fn metadata<S: ReadSource + ?Sized>(source: &S) -> Result<(u32, u64)> {
    let length = source.len()?;
    if length > read::MAX_BYTES as u64 {
        return Err(Error::new(0, "Ogg Vorbis 数据超过读取上限"));
    }
    stream_end(source, length as usize)
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
