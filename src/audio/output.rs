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
    pub signal: u32,
}

pub(super) enum Action {
    Replace(Box<SoundChannel>),
    Start(u32, [i64; 2]),
    Pause,
    Stop,
    Count(u32, [i64; 2]),
    Gain(i32, i32),
    Signal(bool),
}

enum Update {
    Channel {
        channel: usize,
        generation: u64,
        action: Action,
    },
    Movie(u32, Option<crate::movie::AudioOutput>),
    Swap(usize, usize),
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
                signal: channel
                    .source
                    .as_ref()
                    .map_or(0, |source| channel.playback.signal_level(source)),
                ..Status::default()
            }))
        });
        let sound = SoundState {
            channels: self.channels.clone(),
            movies: self.movies.clone(),
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
                status.signal = sound
                    .source
                    .as_ref()
                    .map_or(0, |source| sound.playback.signal_level(source));
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
                let sound = &self.channels[channel];
                status.signal = sound
                    .source
                    .as_ref()
                    .map_or(0, |source| sound.playback.signal_level(source));
            }
            Action::Pause => status.playing = false,
            Action::Stop => {
                status.playing = false;
                status.position = 0;
                status.completed = 0;
                let sound = &self.channels[channel];
                status.signal = sound
                    .source
                    .as_ref()
                    .map_or(0, |source| sound.playback.signal_level(source));
            }
            Action::Count(count, _) => {
                status.remaining = *count;
                status.completed = 0;
            }
            Action::Gain(_, _) => (),
            Action::Signal(_) => (),
        }
        let _ = link.sender.send(Update::Channel {
            channel,
            generation: status.generation,
            action,
        });
    }

    /// 设置独立电影音轨；不占用脚本的六十四个物理声音通道。
    pub fn set_movie_output(&mut self, movie: Option<crate::movie::AudioOutput>) {
        self.set_movie_track(0, movie);
    }

    /// 按播放器句柄登记独立音轨；零号为 set_movie_output 使用的默认音轨。
    /// 替换或移除一个句柄不改变其他音轨，连接设备后同样有效。
    pub fn set_movie_track(&mut self, handle: u32, movie: Option<crate::movie::AudioOutput>) {
        if let Some(output) = &movie {
            self.movies.insert(handle, output.clone());
        } else {
            self.movies.remove(&handle);
        }
        if let Some(link) = &self.output {
            let _ = link.sender.send(Update::Movie(handle, movie));
        }
    }

    pub(super) fn output_replace(&mut self, channel: usize) {
        if let Some(link) = &mut self.output {
            let mut sound = self.channels[channel].clone();
            sound.progress = None;
            link.gains[channel] = [sound.pcm_volume, sound.pcm_pan];
            self.output_action(channel, Action::Replace(Box::new(sound)));
        }
    }

    pub(super) fn output_swap(&mut self, first: usize, second: usize) {
        if let Some(link) = &mut self.output {
            link.progress.swap(first, second);
            link.gains.swap(first, second);
            let _ = link.sender.send(Update::Swap(first, second));
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
    /// Apply pending controls, write interleaved left/right `f32` PCM in `[-1, 1]`,
    /// and report playback progress to the runtime. The rate is in Hz and the
    /// output must contain an even number of samples. Does not wait for decoded data.
    pub fn render(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        for update in self.receiver.try_iter() {
            let (channel, generation, action) = match update {
                Update::Channel {
                    channel,
                    generation,
                    action,
                } => (channel, generation, action),
                Update::Movie(handle, movie) => {
                    self.sound.set_movie_track(handle, movie);
                    continue;
                }
                Update::Swap(first, second) => {
                    self.sound.channels.swap(first, second);
                    self.progress.swap(first, second);
                    self.generations.swap(first, second);
                    continue;
                }
            };
            let sound_channel = &mut self.sound.channels[channel];
            match action {
                Action::Replace(sound) => *sound_channel = *sound,
                Action::Start(count, range) => {
                    if sound_channel.playback.playing || !sound_channel.resampler.paused {
                        sound_channel.resampler = super::mix::Resampler::default();
                    }
                    sound_channel.resampler.paused = false;
                    if let Some(pcm) = &sound_channel.source {
                        sound_channel.playback.start(pcm, count, range)?;
                    }
                }
                Action::Pause => {
                    sound_channel.playback.pause();
                    sound_channel.resampler.paused = true;
                }
                Action::Stop => {
                    sound_channel.resampler = super::mix::Resampler::default();
                    if let Some(pcm) = &sound_channel.source {
                        sound_channel.playback.stop(pcm)?;
                    }
                }
                Action::Count(count, range) => sound_channel.playback.change_count(count, range),
                Action::Gain(volume, pan) => {
                    sound_channel.pcm_volume = volume;
                    sound_channel.pcm_pan = pan;
                }
                Action::Signal(enabled) => {
                    sound_channel.signal_enabled = enabled;
                    sound_channel.playback.set_signal_enabled(enabled);
                }
            }
            self.generations[channel] = generation;
        }
        self.sound.mix(sample_rate, output, false)?;
        for (index, channel) in self.sound.channels.iter().enumerate() {
            if let Ok(mut status) = self.progress[index].try_lock() {
                if status.generation == self.generations[index] {
                    status.playing = channel.is_playing();
                    status.position = channel.position_frames();
                    status.remaining = channel.remaining_plays();
                    status.completed = channel.completed_passes();
                    status.signal = channel
                        .source
                        .as_ref()
                        .map_or(0, |source| channel.playback.signal_level(source));
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
