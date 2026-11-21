use crate::{
    Error, Result,
    host::{
        graphics::pixel_length,
        media::DecodeMode,
        movie::{MovieDecoder, MovieDecoderFactory, MovieFrame, MovieMetadata},
        storage::SharedSource,
    },
    read,
};
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
    first: Option<MovieFrame>,
    video: VecDeque<MovieFrame>,
    audio: VecDeque<Chunk>,
    error: Option<Error>,
    gain: f32,
}

struct Shared {
    inline: Option<Mutex<Option<Decode>>>,
    state: Mutex<State>,
    changed: Condvar,
    metadata: MovieMetadata,
    lead: f64,
    video_capacity: usize,
}

/// Movie playback state. The host consumes video and audio separately.
pub struct Player {
    source: SharedSource,
    factory: Arc<dyn MovieDecoderFactory>,
    initial: Option<Box<dyn MovieDecoder>>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

/// Shared audio output for a single consumer.
#[derive(Clone)]
pub struct AudioOutput(Arc<Shared>);

impl Player {
    /// Prepare playback with the given factory, also used to reopen the source.
    pub fn new(source: SharedSource, factory: Arc<dyn MovieDecoderFactory>) -> Result<Self> {
        Self::with_decode_mode(source, factory, DecodeMode::Threaded)
    }

    /// Prepare playback using the selected decode policy.
    pub fn with_decode_mode(
        source: SharedSource,
        factory: Arc<dyn MovieDecoderFactory>,
        mode: DecodeMode,
    ) -> Result<Self> {
        let decoder = factory.open(source.clone())?;
        let metadata = decoder.metadata();
        let bytes = pixel_length(metadata.size)?;
        if bytes == 0
            || bytes > 128 * 1024 * 1024
            || !metadata.frame_rate.is_finite()
            || metadata.frame_rate <= 0.
            || !metadata.duration.is_finite()
            || metadata.duration < 0.
        {
            return Err(Error::new(0, "Invalid movie decoder metadata"));
        }
        let capacity = (32 * 1024 * 1024 / bytes).clamp(2, 8);
        Ok(Self {
            source,
            factory,
            initial: Some(decoder),
            shared: Arc::new(Shared {
                inline: mode.uses_inline().then(|| Mutex::new(None)),
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

    pub fn metadata(&self) -> MovieMetadata {
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
    /// 返回前准备少量输出。
    pub fn play(&mut self, count: u32) -> Result<bool> {
        if self.is_playing()? {
            return Ok(false);
        }
        self.stop()?;
        let decoder = match self.initial.take() {
            Some(decoder) => decoder,
            None => {
                let decoder = self.factory.open(self.source.clone())?;
                if decoder.metadata() != self.shared.metadata {
                    return Err(Error::new(0, "Movie decoder metadata changed on reopen"));
                }
                decoder
            }
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
        let decode = Decode::new(decoder, count);
        if let Some(inline) = &self.shared.inline {
            *inline.lock().unwrap_or_else(|e| e.into_inner()) = Some(decode);
            self.shared.pump_inline()?;
            return Ok(true);
        }
        let shared = self.shared.clone();
        self.worker = match std::thread::Builder::new()
            .name("katze-movie".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_thread(decode, &shared)
                }))
                .unwrap_or_else(|_| Err(Error::new(0, "电影解码线程发生 panic")));
                if let Err(error) = result {
                    shared.fail(error);
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

    /// 停止音频与解码，返回停止前是否仍在播放。
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
        if let Some(inline) = &self.shared.inline {
            inline.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| Error::new(0, "电影解码线程异常结束"))?;
        }
        Ok(playing)
    }

    /// 取得当前时刻可显示的最后一帧；宿主持有前次画面直到有新输出。
    pub fn take_video(&mut self) -> Result<Option<MovieFrame>> {
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
        if self.shared.metadata.sample_rate == 0 {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            if state.playing {
                state.clock += seconds;
                finish(&mut state);
                self.shared.changed.notify_all();
            }
        }
        self.shared.pump_inline()
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

impl Shared {
    fn fail(&self, error: Error) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.error = Some(error);
        state.playing = false;
        state.ready = true;
        self.changed.notify_all();
    }

    fn pump_inline(&self) -> Result<()> {
        let Some(inline) = &self.inline else {
            return Ok(());
        };
        let mut inline = inline.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(decode) = inline.as_mut() {
            {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if !state.playing {
                    *inline = None;
                    break;
                }
                if state.decoded_until >= state.clock + self.lead {
                    break;
                }
            }
            match decode.step(self) {
                Ok(true) => (),
                Ok(false) => *inline = None,
                Err(error) => {
                    *inline = None;
                    self.fail(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

fn decode_thread(mut decode: Decode, shared: &Shared) -> Result<()> {
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
        if !decode.step(shared)? {
            return Ok(());
        }
    }
}

struct Decode {
    decoder: Box<dyn MovieDecoder>,
    remaining: Option<u32>,
    origin: f64,
    end: f64,
    first: bool,
}

impl Decode {
    fn new(decoder: Box<dyn MovieDecoder>, count: u32) -> Self {
        Self {
            decoder,
            remaining: (!matches!(count, 0 | u32::MAX)).then_some(count),
            origin: 0.,
            end: 0.,
            first: true,
        }
    }

    fn step(&mut self, shared: &Shared) -> Result<bool> {
        let previous = self.decoder.time();
        let decoded = self
            .decoder
            .advance(1. / shared.metadata.frame_rate.max(60.))?;
        let time = self.decoder.time();
        if !time.is_finite() || time < 0. || (!decoded.ended && time <= previous) {
            return Err(Error::new(0, "Movie decoder did not advance"));
        }
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.playing {
            return Ok(false);
        }
        if let Some(mut frame) = decoded.video {
            if !frame.time.is_finite()
                || frame.time < 0.
                || frame.surface.size() != shared.metadata.size
            {
                return Err(Error::new(0, "Invalid movie decoder frame"));
            }
            self.end = self.end.max(frame.time + 1. / shared.metadata.frame_rate);
            frame.time += self.origin;
            if self.first {
                state.first = Some(frame);
                self.first = false;
            } else {
                state.video.push_back(frame);
                while state.video.len() > shared.video_capacity {
                    state.video.pop_front();
                }
            }
        }
        if let Some(audio) = decoded.audio {
            if shared.metadata.sample_rate == 0
                || !audio.time.is_finite()
                || audio.time < 0.
                || !audio.samples.len().is_multiple_of(2)
            {
                return Err(Error::new(0, "Invalid movie decoder audio"));
            }
            self.end = self.end.max(
                audio.time + audio.samples.len() as f64 / 2. / shared.metadata.sample_rate as f64,
            );
            state.audio.push_back(Chunk {
                time: self.origin + audio.time,
                samples: audio.samples,
            });
        }
        state.decoded_until = self.origin + time;
        shared.changed.notify_all();
        if decoded.ended {
            if self.remaining == Some(1) {
                state.end = Some(self.origin + self.end);
                state.ready = true;
                finish(&mut state);
                shared.changed.notify_all();
                return Ok(false);
            }
            if let Some(count) = &mut self.remaining {
                *count -= 1;
            }
            if self.end <= 0. {
                return Err(Error::new(
                    0,
                    "Movie decoder produced no frames or audio to loop",
                ));
            }
            self.origin += self.end;
            self.end = 0.;
            drop(state);
            self.decoder.rewind()?;
        }
        Ok(true)
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
                // At the decode watermark, the decoder waits for playback to advance.
                // With no queued samples this part of the movie is silent.
                if state.end.is_some() || state.decoded_until >= state.clock + self.0.lead {
                    break;
                }
                if !wait {
                    self.0.changed.notify_all();
                    return Ok(());
                }
                if self.0.inline.is_some() {
                    drop(state);
                    self.0.pump_inline()?;
                    state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                } else {
                    self.0.changed.notify_all();
                    state = self
                        .0
                        .changed
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                }
            }
            state.clock += 1. / sample_rate as f64;
            finish(&mut state);
        }
        self.0.changed.notify_all();
        Ok(())
    }
}
