use super::{SoundChannel, SoundState};
use crate::{Error, Result};
use std::sync::{Arc, Mutex, mpsc};

#[derive(Default)]
pub(super) struct Status {
    generation: u64,
    pub playing: bool,
    pub position: usize,
    pub remaining: u32,
    pub completed: u64,
}

pub(super) enum Action {
    Replace(Box<SoundChannel>),
    Start(u32, [i64; 2]),
    Pause,
    Stop,
    Count(u32, [i64; 2]),
    Gain(i32, i32),
}

struct Update {
    channel: usize,
    generation: u64,
    action: Action,
}

pub(super) struct Link {
    sender: mpsc::Sender<Update>,
    progress: [Arc<Mutex<Status>>; 64],
    gains: [[i32; 2]; 64],
}

/// 可交给设备音频线程的独立混音器；渲染期间不访问文件或游戏运行时。
pub struct Mixer {
    sound: SoundState,
    receiver: mpsc::Receiver<Update>,
    progress: [Arc<Mutex<Status>>; 64],
    generations: [u64; 64],
}

impl SoundState {
    /// 将采样消费交给独立混音器；任务通道继续接收控制并查询设备进度。
    /// 同一声音存储只能建立一次连接。
    pub fn connect_output(&mut self) -> Result<Mixer> {
        if self.output.is_some() {
            return Err(Error::new(0, "声音输出已连接"));
        }
        let (sender, receiver) = mpsc::channel();
        let progress = std::array::from_fn(|index| {
            let channel = &self.channels[index];
            Arc::new(Mutex::new(Status {
                playing: channel.is_playing(),
                position: channel.position_frames(),
                remaining: channel.remaining_plays(),
                completed: channel.completed_passes(),
                ..Status::default()
            }))
        });
        let sound = SoundState {
            channels: self.channels.clone(),
            ..SoundState::default()
        };
        for (channel, progress) in self.channels.iter_mut().zip(&progress) {
            channel.progress = Some(progress.clone());
        }
        self.output = Some(Link {
            sender,
            progress: progress.clone(),
            gains: self
                .channels
                .each_ref()
                .map(|channel| [channel.pcm_volume, channel.pcm_pan]),
        });
        Ok(Mixer {
            sound,
            receiver,
            progress,
            generations: [0; 64],
        })
    }

    pub(super) fn output_action(&self, channel: usize, action: Action) {
        let Some(link) = &self.output else { return };
        let mut status = link.progress[channel]
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        status.generation = status.generation.wrapping_add(1);
        match &action {
            Action::Replace(sound) => {
                status.playing = sound.playback.playing;
                status.position = sound.playback.position;
                status.remaining = sound.playback.remaining;
                status.completed = sound.playback.completed;
            }
            Action::Start(count, _) => {
                if status.playing || status.completed != 0 {
                    status.position = 0;
                }
                status.playing = self.channels[channel].playback.playing;
                status.remaining = *count;
                status.completed = 0;
            }
            Action::Pause => status.playing = false,
            Action::Stop => {
                status.playing = false;
                status.position = 0;
                status.completed = 0;
            }
            Action::Count(count, _) => {
                status.remaining = *count;
                status.completed = 0;
            }
            Action::Gain(_, _) => (),
        }
        let _ = link.sender.send(Update {
            channel,
            generation: status.generation,
            action,
        });
    }

    pub(super) fn output_replace(&mut self, channel: usize) {
        if let Some(link) = &mut self.output {
            let mut sound = self.channels[channel].clone();
            sound.progress = None;
            link.gains[channel] = [sound.pcm_volume, sound.pcm_pan];
            self.output_action(channel, Action::Replace(Box::new(sound)));
        }
    }

    pub(super) fn sync_output(&mut self) {
        let Some(link) = &self.output else { return };
        for (channel, progress) in self.channels.iter_mut().zip(&link.progress) {
            let status = progress.lock().unwrap_or_else(|error| error.into_inner());
            channel.playback.playing = status.playing;
            channel.playback.position = status.position;
            channel.playback.remaining = status.remaining;
            channel.playback.completed = status.completed;
        }
    }

    pub(super) fn output_gains(&mut self) {
        for index in 0..self.channels.len() {
            let gain = [
                self.channels[index].pcm_volume,
                self.channels[index].pcm_pan,
            ];
            let Some(link) = &mut self.output else { return };
            if link.gains[index] != gain {
                link.gains[index] = gain;
                self.output_action(index, Action::Gain(gain[0], gain[1]));
            }
        }
    }
}

impl Mixer {
    /// 消费已提交的控制，覆写交错的立体声采样，并发布新的播放进度。
    pub fn render(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        for update in self.receiver.try_iter() {
            let channel = &mut self.sound.channels[update.channel];
            match update.action {
                Action::Replace(sound) => *channel = *sound,
                Action::Start(count, range) => {
                    if channel.playback.playing || !channel.resampler.paused {
                        channel.resampler = super::mix::Resampler::default();
                    }
                    channel.resampler.paused = false;
                    if let Some(pcm) = &channel.source {
                        channel.playback.start(pcm, count, range)?;
                    }
                }
                Action::Pause => {
                    channel.playback.pause();
                    channel.resampler.paused = true;
                }
                Action::Stop => {
                    channel.resampler = super::mix::Resampler::default();
                    if let Some(pcm) = &channel.source {
                        channel.playback.stop(pcm)?;
                    }
                }
                Action::Count(count, range) => channel.playback.change_count(count, range),
                Action::Gain(volume, pan) => {
                    channel.pcm_volume = volume;
                    channel.pcm_pan = pan;
                }
            }
            self.generations[update.channel] = update.generation;
        }
        self.sound.mix(sample_rate, output, false)?;
        for (index, channel) in self.sound.channels.iter().enumerate() {
            if let Ok(mut status) = self.progress[index].try_lock() {
                if status.generation == self.generations[index] {
                    status.playing = channel.is_playing();
                    status.position = channel.position_frames();
                    status.remaining = channel.remaining_plays();
                    status.completed = channel.completed_passes();
                }
            }
        }
        Ok(())
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        for progress in &self.progress {
            if let Ok(mut status) = progress.try_lock() {
                status.playing = false;
            }
        }
    }
}
