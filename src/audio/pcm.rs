use super::Pcm;
use crate::host::storage::ReadSource;
use crate::{Error, Result, read};

/// 解码单个完整 Ogg Vorbis 流；保留采样率、声道顺序及末页的帧数。
pub fn decode_vorbis(bytes: &[u8]) -> Result<Pcm> {
    decode_vorbis_source(bytes)
}

/// 从独立随机读取源解码完整 Ogg Vorbis 流；不持有资源路由或文件路径。
pub fn decode_vorbis_source<S: ReadSource + ?Sized>(source: &S) -> Result<Pcm> {
    let mut decoder = super::VorbisDecoder::new(source)?;
    let rate = decoder.sample_rate();
    let channels = decoder.channels();
    let mut samples = Vec::with_capacity(sample_count(decoder.frames(), channels)?);
    while let Some(packet) = decoder.read_packet()? {
        samples.extend(packet);
    }
    Pcm::new(rate, channels, samples)
}

pub(super) fn sample_count(frames: usize, channels: u8) -> Result<usize> {
    frames
        .checked_mul(channels as usize)
        .filter(|count| *count <= read::MAX_BYTES / 2)
        .ok_or_else(|| Error::new(0, "PCM 数据超过读取上限"))
}
