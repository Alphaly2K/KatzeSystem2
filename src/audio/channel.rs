use super::Pcm;
use super::playback::Playback;

#[derive(Clone, Copy, Default)]
pub(super) struct ChannelClock {
    pub previous: u32,
    pub elapsed: u32,
}

/// 一个物理声音通道持有的资源及装载参数；PCM 保留源格式。
#[derive(Clone)]
pub struct SoundChannel {
    pub(super) clock: ChannelClock,
    pub(super) file: Vec<u8>,
    pub(super) tags: std::collections::BTreeMap<Vec<u8>, u32>,
    pub(super) pcm: Option<Pcm>,
    pub(super) requested_volume: f32,
    pub(super) pcm_volume: i32,
    pub(super) pcm_pan: i32,
    pub(super) distance_range: [f32; 2],
    pub(super) position: [f32; 3],
    pub(super) velocity: [f32; 3],
    pub(super) doppler_factor: f32,
    pub(super) ramps: super::control::Ramps,
    pub(super) playback: Playback,
    pub(super) started_tick: u32,
    pub(super) parameters: [i32; 2],
    pub(super) load_options: u32,
    pub(super) linked: Option<u32>,
    pub(super) loop_range: [i64; 2],
    pub(super) grouped: bool,
    pub(super) allocation_flags: [u32; 2],
    pub(super) progress: Option<std::sync::Arc<std::sync::Mutex<super::output::Status>>>,
    pub(super) resampler: super::mix::Resampler,
}

impl Default for SoundChannel {
    fn default() -> Self {
        Self {
            clock: ChannelClock::default(),
            file: Vec::new(),
            tags: Default::default(),
            pcm: None,
            requested_volume: 0.0,
            pcm_volume: 100,
            pcm_pan: 0,
            distance_range: [100.0; 2],
            position: [0.0; 3],
            velocity: [0.0; 3],
            doppler_factor: 1.0,
            ramps: super::control::Ramps::default(),
            playback: Playback::default(),
            started_tick: u32::MAX,
            parameters: [0; 2],
            load_options: 0,
            linked: None,
            loop_range: [0, -1],
            grouped: false,
            allocation_flags: [0; 2],
            progress: None,
            resampler: super::mix::Resampler::default(),
        }
    }
}

impl SoundChannel {
    pub fn file(&self) -> &[u8] {
        &self.file
    }

    /// 当前资源的原始 TAG 名称和值；停止和卸载不清除此表。
    pub fn tags(&self) -> &std::collections::BTreeMap<Vec<u8>, u32> {
        &self.tags
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

    /// 当前资源的最小／最大声音距离，保留原始浮点位模式。
    pub fn distance_range(&self) -> Option<[f32; 2]> {
        self.pcm.as_ref().map(|_| self.distance_range)
    }

    /// 当前资源的三维位置；二维播放使用其 X／Z 分量计算声像。
    pub fn position(&self) -> Option<[f32; 3]> {
        self.pcm.as_ref().map(|_| self.position)
    }

    /// 当前资源的三维速度，保留原始浮点位模式。
    pub fn velocity(&self) -> Option<[f32; 3]> {
        self.pcm.as_ref().map(|_| self.velocity)
    }

    /// 当前资源保存的 Doppler 系数；设备应用由播放后端负责。
    pub fn doppler_factor(&self) -> Option<f32> {
        self.pcm.as_ref().map(|_| self.doppler_factor)
    }

    pub fn is_playing(&self) -> bool {
        if let Some(progress) = &self.progress {
            return progress
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .playing;
        }
        self.pcm.is_some() && (self.playback.playing || self.resampler.pending())
    }

    /// 由采样读取推进的位置；流式来源返回环形缓冲区内的采样帧位置。
    pub fn position_frames(&self) -> usize {
        if let Some(progress) = &self.progress {
            return progress
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .position;
        }
        self.playback.position
    }

    pub fn remaining_plays(&self) -> u32 {
        if let Some(progress) = &self.progress {
            return progress
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remaining;
        }
        self.playback.remaining
    }

    /// 整段来源已消费的遍数；基线流式来源保持零。
    pub fn completed_passes(&self) -> u64 {
        if let Some(progress) = &self.progress {
            return progress
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .completed;
        }
        self.playback.completed
    }

    pub fn started_tick(&self) -> u32 {
        self.started_tick
    }

    pub fn load_parameters(&self) -> [i32; 2] {
        self.parameters
    }

    /// 装载时指定的原始前置静音毫秒数；不改变源 PCM 或时长查询。
    pub fn load_options(&self) -> u32 {
        self.load_options
    }

    pub fn linked_channel(&self) -> Option<u32> {
        self.linked
    }

    pub fn is_grouped(&self) -> bool {
        self.grouped
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
        self.resampler = super::mix::Resampler::default();
        self.ramps.active = 0;
        self.started_tick = u32::MAX;
        self.clock = ChannelClock::default();
        self.loop_range[0] = 0;
    }
}
