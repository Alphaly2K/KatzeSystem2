use super::{Decoder, Frame, Metadata};
use crate::{Error, Result, io::SharedSource, read};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};

struct Chunk {
    time: f64,
    samples: Vec<f32>,
}

#[derive(Default)]
struct State {
    playing: bool,
    ready: bool,
    clock: f64,
    decoded_until: f64,
    end: Option<f64>,
    first: Option<Frame>,
    video: VecDeque<Frame>,
    audio: VecDeque<Chunk>,
    error: Option<Error>,
    gain: f32,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    metadata: Metadata,
    lead: f64,
    video_capacity: usize,
}

/// 在独立线程解码的电影播放器；画面和音频由宿主分别取走。
pub struct Player {
    source: SharedSource,
    initial: Option<Decoder>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

/// 单个消费端的共享音频入口；解码工作始终由播放器线程完成。
#[derive(Clone)]
pub struct AudioOutput(Arc<Shared>);

impl Player {
    pub fn new(source: SharedSource) -> Result<Self> {
        let decoder = Decoder::new(source.clone())?;
        let metadata = decoder.metadata();
        let bytes = metadata.size[0] as usize * metadata.size[1] as usize * 4;
        let capacity = (32 * 1024 * 1024 / bytes).clamp(2, 8);
        Ok(Self {
            source,
            initial: Some(decoder),
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    gain: 1.,
                    ..State::default()
                }),
                changed: Condvar::new(),
                metadata,
                lead: ((capacity - 1) as f64 / metadata.frame_rate).min(0.25),
                video_capacity: capacity,
            }),
            worker: None,
        })
    }

    pub fn metadata(&self) -> Metadata {
        self.shared.metadata
    }
    pub fn audio_output(&self) -> AudioOutput {
        AudioOutput(self.shared.clone())
    }

    pub fn is_playing(&self) -> Result<bool> {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.error.clone().map_or(Ok(state.playing), Err)
    }

    pub fn time(&self) -> f64 {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clock
    }

    pub fn set_volume(&mut self, volume: i32) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .gain = volume.clamp(0, 100) as f32 / 100.;
    }

    /// 已在播放时返回 false；零和 u32::MAX 均循环播放，其他值为完整播放次数。
    /// 返回前准备少量输出；后续文件读取与解码不占用调用线程。
    pub fn play(&mut self, count: u32) -> Result<bool> {
        if self.is_playing()? {
            return Ok(false);
        }
        self.stop()?;
        let decoder = match self.initial.take() {
            Some(decoder) => decoder,
            None => Decoder::new(self.source.clone())?,
        };
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            let gain = state.gain;
            *state = State {
                playing: true,
                gain,
                ..State::default()
            };
        }
        let shared = self.shared.clone();
        self.worker = match std::thread::Builder::new()
            .name("katze-movie".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode(decoder, &shared, count)
                }))
                .unwrap_or_else(|_| Err(Error::new(0, "电影解码线程发生 panic")));
                if let Err(error) = result {
                    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.error = Some(error);
                    state.playing = false;
                    state.ready = true;
                    shared.changed.notify_all();
                }
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                self.shared
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .playing = false;
                return Err(Error::new(0, format!("电影解码线程创建失败：{error}")));
            }
        };
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        while !state.ready {
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
        state.error.clone().map_or(Ok(true), Err)
    }

    /// 停止音频与解码并等待线程退出，返回停止前是否仍在播放。
    pub fn stop(&mut self) -> Result<bool> {
        let playing = {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            let playing = state.playing;
            state.playing = false;
            state.audio.clear();
            state.video.clear();
            state.first = None;
            self.shared.changed.notify_all();
            playing
        };
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| Error::new(0, "电影解码线程异常结束"))?;
        }
        Ok(playing)
    }

    /// 取得当前时刻可显示的最后一帧；宿主持有前次画面直到有新输出。
    pub fn take_video(&mut self) -> Result<Option<Frame>> {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        let mut frame = state.first.take();
        while state
            .video
            .front()
            .is_some_and(|frame| frame.time <= state.clock + 1e-9)
        {
            frame = state.video.pop_front();
        }
        Ok(frame)
    }

    /// 无音轨时由宿主提供时钟；有音轨时以实际采样消费为准。
    pub fn advance(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0. {
            return Err(Error::new(0, "电影时钟增量无效"));
        }
        if self.shared.metadata.sample_rate != 0 {
            return Ok(());
        }
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if state.playing {
            state.clock += seconds;
            finish(&mut state);
            self.shared.changed.notify_all();
        }
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn finish(state: &mut State) {
    if state.end.is_some_and(|end| state.clock >= end - 1e-9) {
        state.playing = false;
    }
}

fn decode(mut decoder: Decoder, shared: &Shared, count: u32) -> Result<()> {
    let mut remaining = (!matches!(count, 0 | u32::MAX)).then_some(count);
    let mut origin = 0.;
    let mut end = 0f64;
    let mut first = true;
    let tick = 1. / shared.metadata.frame_rate.max(60.);
    loop {
        {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while state.playing && state.decoded_until >= state.clock + shared.lead {
                state.ready = true;
                shared.changed.notify_all();
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if !state.playing {
                return Ok(());
            }
        }
        let decoded = decoder.advance(tick)?;
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.playing {
            return Ok(());
        }
        if let Some(mut frame) = decoded.video {
            end = end.max(frame.time + 1. / shared.metadata.frame_rate);
            frame.time += origin;
            if first {
                state.first = Some(frame);
                first = false;
            } else {
                state.video.push_back(frame);
                while state.video.len() > shared.video_capacity {
                    state.video.pop_front();
                }
            }
        }
        if let Some(audio) = decoded.audio {
            end = end.max(
                audio.time + audio.samples.len() as f64 / 2. / shared.metadata.sample_rate as f64,
            );
            state.audio.push_back(Chunk {
                time: origin + audio.time,
                samples: audio.samples,
            });
        }
        state.decoded_until = origin + decoder.time();
        shared.changed.notify_all();
        if decoded.ended {
            if remaining == Some(1) {
                state.end = Some(origin + end);
                state.ready = true;
                finish(&mut state);
                shared.changed.notify_all();
                return Ok(());
            }
            if let Some(count) = &mut remaining {
                *count -= 1;
            }
            origin += end;
            end = 0.;
            drop(state);
            decoder.rewind()?;
        }
    }
}

impl AudioOutput {
    /// 将电影叠加到立体声输出；暂未准备好时保持剩余输出并等待下一次设备回调。
    pub fn mix_stereo(&self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        self.mix(sample_rate, output, false)
    }

    pub(crate) fn mix(&self, sample_rate: u32, output: &mut [f32], wait: bool) -> Result<()> {
        if sample_rate == 0 || !output.len().is_multiple_of(2) || output.len() > read::MAX_BYTES / 4
        {
            return Err(Error::new(0, "电影音频输出格式或容量无效"));
        }
        let mut state = if wait {
            self.0.state.lock().unwrap_or_else(|e| e.into_inner())
        } else {
            match self.0.state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
            }
        };
        let rate = self.0.metadata.sample_rate as f64;
        if rate == 0. {
            return Ok(());
        }
        for frame in output.chunks_exact_mut(2) {
            loop {
                if let Some(error) = &state.error {
                    return Err(error.clone());
                }
                if !state.playing {
                    return Ok(());
                }
                while state.audio.front().is_some_and(|chunk| {
                    state.clock + 1e-9 >= chunk.time + chunk.samples.len() as f64 / 2. / rate
                }) {
                    state.audio.pop_front();
                }
                if let Some(chunk) = state.audio.front() {
                    if state.clock + 1e-9 >= chunk.time {
                        let position = ((state.clock - chunk.time) * rate).max(0.);
                        let index = position.floor() as usize;
                        let fraction = (position - index as f64) as f32;
                        let index = index.min(chunk.samples.len() / 2 - 1);
                        for (side, output) in frame.iter_mut().enumerate() {
                            let a = chunk.samples[index * 2 + side];
                            let b = if let Some(&sample) = chunk.samples.get((index + 1) * 2 + side)
                            {
                                sample
                            } else {
                                state
                                    .audio
                                    .get(1)
                                    .filter(|next| {
                                        (next.time
                                            - chunk.time
                                            - chunk.samples.len() as f64 / 2. / rate)
                                            .abs()
                                            < 1e-6
                                    })
                                    .map_or(a, |next| next.samples[side])
                            };
                            *output += (a + (b - a) * fraction) * state.gain;
                        }
                    }
                    break;
                }
                if state.end.is_some() {
                    break;
                }
                if !wait {
                    self.0.changed.notify_all();
                    return Ok(());
                }
                self.0.changed.notify_all();
                state = self
                    .0
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            state.clock += 1. / sample_rate as f64;
            finish(&mut state);
        }
        self.0.changed.notify_all();
        Ok(())
    }
}
