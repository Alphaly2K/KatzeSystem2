use crate::host::storage::{ReadSource, SharedSource, SourceReader};
use crate::{Error, Result, read};
use lewton::{
    audio::get_decoded_sample_count,
    inside_ogg::OggStreamReader,
    samples::{InterleavedSamples, Samples},
};

/// 单个 Ogg Vorbis 流的逐包解码器；构造读取格式与末页长度，不解码音频包。
/// 返回交错的有符号 16 位采样，最后一包裁到末页声明的帧数。
pub struct VorbisDecoder<'a, S: ReadSource + ?Sized> {
    decoder: OggStreamReader<SourceReader<'a, S>>,
    source: SourceReader<'a, S>,
    granules: Vec<u64>,
    seek_points: Option<Vec<SeekPoint>>,
    serial: u32,
    frames: usize,
    decoded: usize,
    finished: bool,
    pending: Option<Vec<i16>>,
}

struct Metadata {
    serial: u32,
    frames: u64,
    granules: Vec<u64>,
}

#[derive(Clone, Copy)]
struct SeekPoint {
    granule: u64,
    frame: usize,
}

struct Packet {
    samples: Vec<i16>,
    channels: usize,
}

impl Samples for Packet {
    fn num_samples(&self) -> usize {
        self.samples.len() / self.channels
    }

    fn truncate(&mut self, limit: usize) {
        self.samples.truncate(limit * self.channels);
    }

    fn from_floats(floats: Vec<Vec<f32>>) -> Self {
        let channels = floats.len();
        if !matches!(channels, 1 | 2) {
            let packet = InterleavedSamples::from_floats(floats);
            return Self {
                samples: packet.samples,
                channels,
            };
        }
        let to_i16 = |value: f32| (value * 32768.0) as i16;
        let mut floats = floats.into_iter();
        let left = floats.next().unwrap();
        let samples = match floats.next() {
            None => left.into_iter().map(to_i16).collect(),
            Some(right) => left
                .into_iter()
                .zip(right)
                .flat_map(|(left, right)| [to_i16(left), to_i16(right)])
                .collect(),
        };
        Self { samples, channels }
    }
}

impl<'a, S: ReadSource + ?Sized> VorbisDecoder<'a, S> {
    pub fn new(source: &'a S) -> Result<Self> {
        Self::from_reader(SourceReader::new(source), metadata(source)?)
    }

    fn from_reader(reader: SourceReader<'a, S>, metadata: Metadata) -> Result<Self> {
        let source = reader.reopen();
        let decoder = OggStreamReader::new(reader).map_err(decode_error)?;
        let rate = decoder.ident_hdr.audio_sample_rate;
        let channels = decoder.ident_hdr.audio_channels;
        if rate == 0 || !matches!(channels, 1 | 2) {
            return Err(Error::new(0, "Ogg Vorbis 需要非零采样率及单声道或双声道"));
        }
        let frames = usize::try_from(metadata.frames)
            .ok()
            .filter(|frames| frames.checked_mul(channels as usize * 2).is_some())
            .ok_or_else(|| Error::new(0, "Ogg Vorbis 声明采样长度超出可寻址范围"))?;
        Ok(Self {
            decoder,
            source,
            granules: metadata.granules,
            seek_points: None,
            serial: metadata.serial,
            frames,
            decoded: 0,
            finished: false,
            pending: None,
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
        if let Some(packet) = self.pending.take() {
            return Ok(Some(packet));
        }
        if self.finished {
            return Ok(None);
        }
        let expected = self.frames * self.channels() as usize;
        loop {
            let Some(Packet { mut samples, .. }) = self
                .decoder
                .read_dec_packet_generic::<Packet>()
                .map_err(decode_error)?
            else {
                if self.decoded < expected {
                    return Err(Error::new(0, "Ogg Vorbis 采样不足末页声明的帧数"));
                }
                self.finished = true;
                return Ok(None);
            };
            if self.decoder.stream_serial() != self.serial {
                return Err(Error::new(0, "Ogg Vorbis 不支持串联或混合逻辑流"));
            }
            let count = samples.len();
            let next = self
                .decoded
                .checked_add(count)
                .ok_or_else(|| Error::new(0, "Ogg Vorbis 采样位置溢出"))?;
            samples.truncate(expected.saturating_sub(self.decoded));
            self.decoded = next.min(expected);
            if !samples.is_empty() {
                return Ok(Some(samples));
            }
        }
    }

    /// 定位到指定采样帧；下一包从该帧开始，定位期间执行读取和解码。
    pub fn seek_frame(&mut self, frame: usize) -> Result<()> {
        if frame > self.frames {
            return Err(Error::new(0, "Ogg Vorbis 定位超出采样帧范围"));
        }
        self.pending = None;
        self.finished = frame == self.frames;
        let target = frame * self.channels() as usize;
        if self.finished {
            self.decoded = target;
            return Ok(());
        }
        if self.seek_points.is_none() && self.granules.first().is_some_and(|&g| g < frame as u64) {
            self.seek_points = Some(self.index_seek_points()?);
        }
        let points = self.seek_points.as_deref().unwrap_or_default();
        let previous = points.partition_point(|point| point.frame < frame);
        if let Some(point) = previous.checked_sub(1).map(|index| points[index]) {
            self.decoder
                .seek_absgp_pg(point.granule)
                .map_err(decode_error)?;
            // 先完成前一页，使重叠窗口及绝对采样位置都已就绪。
            loop {
                if self
                    .decoder
                    .read_dec_packet_generic::<Packet>()
                    .map_err(decode_error)?
                    .is_none()
                {
                    return Err(Error::new(0, "Ogg Vorbis 定位前置页提前结束"));
                }
                if self.decoder.stream_serial() != self.serial {
                    return Err(Error::new(0, "Ogg Vorbis 不支持串联或混合逻辑流"));
                }
                if let Some(end) = self.decoder.get_last_absgp() {
                    if end == point.granule {
                        self.decoded = point.frame * self.channels() as usize;
                    } else {
                        self.restart()?;
                    }
                    break;
                }
            }
        } else {
            self.restart()?;
        }
        while self.decoded < target {
            let Some(mut packet) = self.read_packet()? else {
                return Err(Error::new(0, "Ogg Vorbis 定位采样不足"));
            };
            if self.decoded > target {
                let skip = packet.len() - (self.decoded - target);
                packet.drain(..skip);
                self.pending = Some(packet);
            }
        }
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        self.decoder = OggStreamReader::new(self.source.reopen()).map_err(decode_error)?;
        self.decoded = 0;
        Ok(())
    }

    fn index_seek_points(&self) -> Result<Vec<SeekPoint>> {
        let mut reader = OggStreamReader::new(self.source.reopen())
            .map_err(decode_error)?
            .into_inner();
        let mut points = Vec::new();
        let mut frame = 0usize;
        let mut primed = false;
        let (ident, setup) = (&self.decoder.ident_hdr, &self.decoder.setup_hdr);
        // A page's granule locates the encoded page. Packet durations locate its PCM.
        while let Some(packet) = reader.read_packet().map_err(|e| decode_error(e.into()))? {
            if packet.last_in_stream() {
                break;
            }
            if primed {
                // Lewton's packet-size helper can panic on an invalid mode index.
                let count = std::panic::catch_unwind(|| {
                    get_decoded_sample_count(ident, setup, &packet.data)
                })
                .map_err(|_| Error::new(0, "Ogg Vorbis 音频包模式无效"))?
                .map_err(|e| decode_error(e.into()))?;
                frame = frame
                    .checked_add(count)
                    .ok_or_else(|| Error::new(0, "Ogg Vorbis 采样位置溢出"))?;
            }
            primed = true;
            let granule = packet.absgp_page();
            if packet.last_in_page() && granule != 0 && granule != u64::MAX {
                let index = self.granules.partition_point(|&end| end < granule);
                if self.granules.get(index + 1) != Some(&granule) {
                    points.push(SeekPoint { granule, frame });
                }
            }
        }
        Ok(points)
    }
}

impl VorbisDecoder<'static, dyn ReadSource> {
    /// 持有共享来源，解码状态及其读取游标可以一起移交预取线程。
    pub fn from_shared(source: SharedSource) -> Result<Self> {
        let metadata = metadata(source.as_ref())?;
        Self::from_reader(SourceReader::from_shared(source), metadata)
    }
}

fn decode_error(error: lewton::VorbisError) -> Error {
    Error::new(0, format!("Ogg Vorbis 解码失败：{error}"))
}

fn metadata<S: ReadSource + ?Sized>(source: &S) -> Result<Metadata> {
    let length = source.len()?;
    if length > read::MAX_BYTES as u64 {
        return Err(Error::new(0, "Ogg Vorbis 数据超过读取上限"));
    }
    stream_end(source, length as usize)
}

fn stream_end<S: ReadSource + ?Sized>(source: &S, length: usize) -> Result<Metadata> {
    let mut position = 0;
    let mut serial = None;
    let mut granules = Vec::new();
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
        let end = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if end != u64::MAX && end != 0 {
            if granules.last().is_some_and(|previous| *previous > end) {
                return Err(Error::new(position, "Ogg Vorbis 页面采样位置倒退"));
            }
            if granules.len() >= read::MAX_ITEMS {
                return Err(Error::new(position, "Ogg Vorbis 页面数量超过读取上限"));
            }
            granules.push(end);
        }
        if header[5] & 4 != 0 {
            if position != length {
                return Err(Error::new(position, "Ogg 结束页后仍有数据"));
            }
            return Ok(Metadata {
                serial: current,
                frames: end,
                granules,
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_matches_lewton_clipping_channel_order_and_frame_truncation() {
        let left = vec![f32::NEG_INFINITY, -1., -0.5, -0., 0.99999, 1., 2., f32::NAN];
        let right = left.iter().rev().map(|sample| -*sample).collect::<Vec<_>>();
        for channels in [vec![left.clone()], vec![left, right]] {
            let mut baseline = InterleavedSamples::<i16>::from_floats(channels.clone());
            let mut packet = Packet::from_floats(channels);
            assert_eq!(packet.num_samples(), baseline.num_samples());
            assert_eq!(packet.samples, baseline.samples);
            packet.truncate(5);
            baseline.truncate(5);
            assert_eq!(packet.num_samples(), 5);
            assert_eq!(packet.samples, baseline.samples);
        }
    }
}
