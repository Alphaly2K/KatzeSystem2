use super::{SoundState, output::Action};

impl SoundState {
    /// 启用物理通道的信号测量；逻辑通道先沿当前备用通道解析。
    pub fn set_signal_enabled(&mut self, channel: u32, enabled: bool) {
        if let Some(index) = self.property_channel(channel) {
            self.channels[index].signal_enabled = enabled;
            self.channels[index].playback.set_signal_enabled(enabled);
            self.output_action(index, Action::Signal(enabled));
        }
    }

    /// 返回当前播放窗口的原始 PCM 变化量，独立于音量和声像。
    pub fn signal_level(&self, channel: u32) -> u32 {
        let Some(index) = self.property_channel(channel) else {
            return 0;
        };
        let sound = &self.channels[index];
        if let Some(progress) = &sound.progress {
            return progress
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .signal;
        }
        sound
            .source
            .as_ref()
            .map_or(0, |source| sound.playback.signal_level(source))
    }
}

#[derive(Clone)]
pub(super) struct Signal {
    enabled: bool,
    step: usize,
    values: [u32; 80],
}

impl Default for Signal {
    fn default() -> Self {
        Self {
            enabled: false,
            step: 0,
            values: [0; 80],
        }
    }
}

impl Signal {
    pub(super) fn new(rate: u32, channels: u8) -> Self {
        Self {
            step: (rate.wrapping_mul(u32::from(channels) * 2) / 2 / 10) as usize,
            ..Self::default()
        }
    }
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn level(&self, position: usize, channels: u8) -> u32 {
        if self.step == 0 {
            return 0;
        }
        self.values
            .get(position * usize::from(channels) * 2 / self.step)
            .copied()
            .unwrap_or(0)
    }
    pub(super) fn refresh(&mut self, samples: &[i16], channels: u8) {
        if !self.enabled {
            return;
        }
        for (index, value) in self.values.iter_mut().enumerate() {
            *value = measure_range(samples, index * self.step, self.step, channels);
        }
    }
    pub(super) fn set_values(&mut self, values: [u32; 80]) {
        self.values = values;
    }
    pub(super) fn step(&self) -> usize {
        self.step
    }
}

pub(super) fn measure_range(samples: &[i16], start: usize, length: usize, channels: u8) -> u32 {
    let end = (start + length).min(samples.len() * 2);
    if start >= end {
        return 0;
    }
    if start.is_multiple_of(2) && end.is_multiple_of(2) {
        return measure(&samples[start / 2..end / 2], channels);
    }
    let read = |position: usize| {
        let low = samples[position / 2].to_le_bytes()[position % 2];
        let high = samples[position.div_ceil(2)].to_le_bytes()[(position + 1) % 2];
        i16::from_le_bytes([low, high])
    };
    let values: Vec<_> = (start..end.saturating_sub(1))
        .step_by(2)
        .map(read)
        .collect();
    measure(&values, channels)
}

pub(super) fn measure(samples: &[i16], channels: u8) -> u32 {
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    if frames == 0 {
        return 0;
    }
    let mut sum = 0u32;
    for pair in samples.windows(channels * 2).step_by(channels) {
        let difference: u32 = pair[..channels]
            .iter()
            .zip(&pair[channels..])
            .map(|(&a, &b)| (i32::from(a) - i32::from(b)).unsigned_abs())
            .sum();
        sum = sum.wrapping_add(difference / channels as u32);
    }
    sum / frames as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{Pcm, Source, playback::Playback};

    #[test]
    fn signal_windows_follow_raw_buffered_and_streamed_samples_on_both_mixing_paths() {
        assert_eq!(measure(&[0, 1000, -1000, 3000], 1), 1750);
        assert_eq!(
            measure(&[0, 0, 1000, -1000, -1000, 1000, 3000, -3000], 2),
            1750
        );
        assert_eq!(measure(&[20000; 8], 1), 0);
        let mut signal = Signal::new(80, 1);
        signal.set_enabled(true);
        signal.refresh(&[0, 1000, -1000, 3000], 1);
        assert_eq!(signal.level(0, 1), 1750);
        signal.set_enabled(false);
        signal.refresh(&[20000; 4], 1);
        assert_eq!(signal.level(0, 1), 1750);
        for whole in [false, true] {
            for connected in [false, true] {
                let source: Source = Pcm::new(80, 1, [0, 4000, 0, 4000].repeat(200))
                    .unwrap()
                    .into();
                let mut playback = Playback::new(&source, whole, 0).unwrap();
                playback.prepare_signal(&source, true).unwrap();
                playback.start(&source, 1, [0, -1]).unwrap();
                let mut sound = SoundState::default();
                sound.channels[3].source = Some(source);
                sound.channels[3].playback = playback;
                sound.channels[3].signal_enabled = true;
                sound.channels[0].linked = Some(3);
                assert_eq!(sound.signal_level(0), 3000);
                let mut mixer = connected.then(|| sound.connect_output().unwrap());
                sound.set_signal_enabled(0, true);
                sound.set_volume(0, 0);
                for index in 0..100 {
                    let mut samples = [0.; 8];
                    if let Some(mixer) = &mut mixer {
                        mixer.render(80, &mut samples).unwrap();
                    } else {
                        sound.mix_stereo(80, &mut samples).unwrap();
                    }
                    assert_eq!(samples, [0.; 8]);
                    assert_eq!(
                        sound.signal_level(0),
                        if whole && index >= 79 { 0 } else { 3000 }
                    );
                }
                sound.set_signal_enabled(0, false);
                assert_eq!(sound.signal_level(0), if whole { 0 } else { 3000 });
                assert_eq!(sound.signal_level(64), 0);
                assert_eq!(sound.signal_level(u32::MAX), 0);
            }
        }
    }
}
