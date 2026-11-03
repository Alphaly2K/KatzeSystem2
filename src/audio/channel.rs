use super::playback::Playback;
use super::{Pcm, decode_vorbis};
use crate::kcs::ResourceServices;
use crate::resource::ResourceMode;
use crate::{Error, Result};

#[derive(Clone, Copy, Default)]
pub(super) struct ChannelClock {
    pub previous: u32,
    pub elapsed: u32,
}

/// 一个物理声音通道持有的资源及装载参数；PCM 保留源格式。
pub struct SoundChannel {
    pub(super) clock: ChannelClock,
    pub(super) file: Vec<u8>,
    pub(super) pcm: Option<Pcm>,
    pub(super) requested_volume: f32,
    pub(super) pcm_volume: i32,
    pub(super) pcm_pan: i32,
    pub(super) playback: Playback,
    pub(super) started_tick: u32,
    pub(super) parameters: [i32; 2],
    pub(super) load_options: u32,
    pub(super) linked: Option<u32>,
    pub(super) loop_range: [i64; 2],
}

impl Default for SoundChannel {
    fn default() -> Self {
        Self {
            clock: ChannelClock::default(),
            file: Vec::new(),
            pcm: None,
            requested_volume: 0.0,
            pcm_volume: 100,
            pcm_pan: 0,
            playback: Playback::default(),
            started_tick: u32::MAX,
            parameters: [0; 2],
            load_options: 0,
            linked: None,
            loop_range: [0, -1],
        }
    }
}

impl SoundChannel {
    pub fn file(&self) -> &[u8] {
        &self.file
    }

    pub fn pcm(&self) -> Option<&Pcm> {
        self.pcm.as_ref()
    }

    /// 任务保存的音量目标；资源尚未装载时也可设置。
    pub fn requested_volume(&self) -> f32 {
        self.requested_volume
    }

    pub fn volume(&self) -> Option<i32> {
        self.pcm.as_ref().map(|_| self.pcm_volume)
    }

    pub fn pan(&self) -> Option<i32> {
        self.pcm.as_ref().map(|_| self.pcm_pan)
    }

    pub fn is_playing(&self) -> bool {
        self.pcm.is_some() && self.playback.playing
    }

    /// 已由采样读取推进的位置；不使用脚本时钟推算。
    pub fn position_frames(&self) -> usize {
        self.playback.position
    }

    pub fn remaining_plays(&self) -> u32 {
        self.playback.remaining
    }

    pub fn completed_passes(&self) -> u64 {
        self.playback.completed
    }

    pub fn started_tick(&self) -> u32 {
        self.started_tick
    }

    pub fn load_parameters(&self) -> [i32; 2] {
        self.parameters
    }

    pub fn load_options(&self) -> u32 {
        self.load_options
    }

    pub fn linked_channel(&self) -> Option<u32> {
        self.linked
    }

    /// 已登记的采样帧循环范围；整段缓冲区播放不应用该范围。
    pub fn loop_range(&self) -> [i64; 2] {
        self.loop_range
    }

    pub(super) fn duration_ms(&self) -> u32 {
        self.pcm.as_ref().map_or(0, |pcm| {
            ((pcm.frames() as f64 / f64::from(pcm.sample_rate())) * 1000.0) as u64 as u32
        })
    }

    pub(super) fn unload(&mut self) {
        self.file.clear();
        self.pcm = None;
        self.playback = Playback::default();
        self.started_tick = u32::MAX;
        self.clock = ChannelClock::default();
        self.loop_range[0] = 0;
    }
}

pub(super) fn load_pcm(services: &mut dyn ResourceServices, file: &[u8]) -> Result<Option<Pcm>> {
    services
        .read(file, ResourceMode::LooseFirst)?
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| decode_vorbis(&bytes))
        .transpose()
}

pub(super) fn check_metadata(services: &mut dyn ResourceServices, file: &[u8]) -> Result<()> {
    let basename = file
        .rsplit(|byte| matches!(byte, b'/' | b'\\'))
        .next()
        .unwrap();
    let stem = without_extension(basename);
    for (category, extension) in [
        (b"tag".as_slice(), None),
        (b"sound".as_slice(), Some(b"plv".as_slice())),
        (b"sound".as_slice(), Some(b"lpb".as_slice())),
        (b"sound".as_slice(), Some(b"lv".as_slice())),
    ] {
        let mut name = services.resolve(category, stem)?;
        name.truncate(
            name.iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len()),
        );
        if let Some(extension) = extension {
            name.truncate(without_extension(&name).len());
            name.push(b'.');
            name.extend(extension);
        }
        if name.len() >= 1024 {
            return Err(Error::new(0, "声音附加资源名称超过缓冲区容量"));
        }
        if services
            .read(&name, ResourceMode::LooseFirst)?
            .is_some_and(|bytes| !bytes.is_empty())
        {
            return Err(Error::new(0, "声音附加资源内容尚未接入"));
        }
    }
    Ok(())
}

fn without_extension(name: &[u8]) -> &[u8] {
    let dot = name
        .iter()
        .rposition(|byte| matches!(byte, b'.' | b'/' | b'\\'))
        .filter(|&position| name[position] == b'.');
    &name[..dot.unwrap_or(name.len())]
}
