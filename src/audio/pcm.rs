use crate::io::ReadSource;
use crate::{Error, Result, read};

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
    let mut decoder = super::VorbisDecoder::new(source)?;
    let rate = decoder.sample_rate();
    let channels = decoder.channels();
    let mut samples = Vec::with_capacity(decoder.frames() * channels as usize);
    while let Some(packet) = decoder.read_packet()? {
        samples.extend(packet);
    }
    Pcm::new(rate, channels, samples)
}
