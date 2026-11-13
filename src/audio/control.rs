use super::SoundChannel;

#[derive(Clone, Default)]
pub(super) struct Ramps {
    pub active: u8,
    pub current: [f32; 2],
    pub target: [f32; 2],
    pub remaining: [f32; 2],
    // 音量先于声像推进，两者沿用同一个更新时间。
    pub previous: u32,
}

impl Ramps {
    fn advance(&mut self, slot: usize, now: u32) -> i32 {
        self.remaining[slot] -= now.wrapping_sub(self.previous) as f32;
        if self.remaining[slot] <= 0.0 {
            self.current[slot] = self.target[slot];
            self.active &= !(1 << slot);
        } else {
            self.current[slot] += (self.target[slot] - self.current[slot]) / self.remaining[slot];
            self.previous = now;
        }
        // 渐变使用有符号 64 位截断后的低 DWORD，与直接设置的 i32 转换分别处理。
        self.current[slot] as i64 as i32
    }
}

impl SoundChannel {
    pub(super) fn update_controls(&mut self, now: u32, stereo: bool) {
        for slot in 0..2 {
            if self.ramps.active & (1 << slot) == 0 {
                continue;
            }
            let value = self.ramps.advance(slot, now);
            if self.source.is_some() {
                if slot == 0 {
                    self.pcm_volume = value;
                } else if stereo {
                    self.pcm_pan = value.clamp(-100, 100);
                }
            }
        }
    }

    pub(super) fn set_position(&mut self, position: [f32; 3]) {
        self.position = position;
        let angle = f64::from(position[0]).atan2(f64::from(position[2])) as f32;
        let pan = f64::from(angle).sin() as f32 * 100.0;
        self.pcm_pan = truncate(pan).clamp(-100, 100);
    }
}

pub(super) fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
