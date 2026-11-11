use katze_system2::{
    Error, Result,
    audio::{Mixer, SoundState},
};
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct Progress {
    frames: AtomicU64,
    nonzero: AtomicU64,
    callbacks: AtomicU64,
    error: Mutex<Option<Error>>,
}

struct Callback {
    mixer: Mixer,
    rate: u32,
    progress: Arc<Progress>,
}

impl AudioCallback for Callback {
    type Channel = f32;

    fn callback(&mut self, output: &mut [f32]) {
        if let Err(error) = self.mixer.render(self.rate, output) {
            output.fill(0.0);
            if let Ok(mut slot) = self.progress.error.try_lock() {
                *slot = Some(error);
            }
        }
        self.progress
            .frames
            .fetch_add((output.len() / 2) as u64, Ordering::Relaxed);
        self.progress.nonzero.fetch_add(
            output.iter().filter(|&&sample| sample != 0.0).count() as u64,
            Ordering::Relaxed,
        );
        self.progress.callbacks.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct Output {
    _device: AudioDevice<Callback>,
    _context: sdl2::Sdl,
    progress: Arc<Progress>,
    report: std::time::Instant,
}

impl Output {
    pub fn new(sound: &mut SoundState) -> Result<Self> {
        let context = sdl2::init().map_err(|message| Error::new(0, message))?;
        let audio = context.audio().map_err(|message| Error::new(0, message))?;
        let mixer = sound.connect_output()?;
        let progress = Arc::new(Progress::default());
        let callback_progress = progress.clone();
        let device = audio
            .open_playback(
                None,
                &AudioSpecDesired {
                    freq: Some(48_000),
                    channels: Some(2),
                    samples: Some(512),
                },
                move |spec| Callback {
                    mixer,
                    rate: spec.freq as u32,
                    progress: callback_progress,
                },
            )
            .map_err(|message| Error::new(0, message))?;
        println!(
            "音频设备：{} / {} Hz",
            audio.current_audio_driver(),
            device.spec().freq
        );
        device.resume();
        Ok(Self {
            _device: device,
            _context: context,
            progress,
            report: std::time::Instant::now(),
        })
    }

    pub fn pump(&mut self) -> Result<()> {
        if let Some(error) = self
            .progress
            .error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            return Err(error);
        }
        if std::env::var_os("KATZE_TRACE_AUDIO").is_some() && self.report.elapsed().as_secs() >= 1 {
            eprintln!(
                "[audio-output] frames={} nonzero={} callbacks={}",
                self.progress.frames.load(Ordering::Relaxed),
                self.progress.nonzero.load(Ordering::Relaxed),
                self.progress.callbacks.load(Ordering::Relaxed)
            );
            self.report = std::time::Instant::now();
        }
        Ok(())
    }
}
