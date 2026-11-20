mod input;
mod inspect;

pub(super) use input::ScriptedDevice;
pub(super) use inspect::inspect_runtime;

use katze_system2::{Result, host::platform::WindowServices};
use std::collections::BTreeMap;

pub(super) struct ProbeWindow {
    pub(super) fullscreen: bool,
}

impl WindowServices for ProbeWindow {
    fn toggle_fullscreen(&mut self) -> Result<bool> {
        self.fullscreen = !self.fullscreen;
        Ok(self.fullscreen)
    }

    fn minimize(&mut self) -> Result<()> {
        Ok(())
    }

    fn confirm_close(&mut self) -> bool {
        true
    }
}

#[derive(Default)]
pub(super) struct AudioStats {
    frames: u64,
    nonzero_frames: u64,
    peak: f32,
    first: Option<u32>,
    last: Option<u32>,
}

impl AudioStats {
    pub(super) fn add(&mut self, round: u32, pcm: &[f32]) {
        for frame in pcm.chunks_exact(2) {
            let peak = frame[0].abs().max(frame[1].abs());
            self.peak = self.peak.max(peak);
            self.frames += 1;
            if peak != 0.0 {
                self.nonzero_frames += 1;
                self.first.get_or_insert(round);
                self.last = Some(round);
            }
        }
    }

    pub(super) fn report(&self) {
        println!(
            "PCM rate=48000 frames={} nonzero_frames={} peak={} first_round={:?} last_round={:?}",
            self.frames, self.nonzero_frames, self.peak, self.first, self.last,
        );
    }
}

pub(super) struct SoundTrace {
    channels: Vec<(u32, Option<PlayingSound>)>,
}

struct PlayingSound {
    name: Vec<u8>,
    gain: Option<i32>,
}

impl SoundTrace {
    pub(super) fn from_env() -> std::result::Result<Option<Self>, Box<dyn std::error::Error>> {
        let Ok(value) = std::env::var("KATZE_PROBE_SOUND") else {
            return Ok(None);
        };
        let mut numbers: Vec<u32> = if value == "all" {
            (0..64).collect()
        } else {
            value
                .split(',')
                .map(|id| id.trim().parse())
                .collect::<std::result::Result<_, _>>()?
        };
        if numbers.iter().any(|&id| id >= 64) {
            return Err("KATZE_PROBE_SOUND expects channel numbers from 0 to 63".into());
        }
        numbers.sort_unstable();
        numbers.dedup();
        Ok(Some(Self {
            channels: numbers.into_iter().map(|id| (id, None)).collect(),
        }))
    }

    pub(super) fn update(
        &mut self,
        frame: u32,
        state: &katze_system2::audio::SoundState,
    ) -> Result<()> {
        for (id, previous) in &mut self.channels {
            let channel = state.channel(*id)?;
            if !channel.is_playing() {
                if previous.take().is_some() {
                    println!("SOUND frame={frame} channel={id} stop");
                }
                continue;
            }
            let gain = channel.volume();
            if previous
                .as_ref()
                .is_none_or(|old| old.name != channel.file() || old.gain != gain)
            {
                let name = channel.file();
                println!(
                    "SOUND frame={frame} channel={id} gain={gain:?} file={:?}",
                    String::from_utf8_lossy(name)
                );
                *previous = Some(PlayingSound {
                    name: name.to_vec(),
                    gain,
                });
            }
        }
        Ok(())
    }
}

pub(super) fn write_shot(
    path: &std::ffi::OsStr,
    [width, height]: [u32; 2],
    pixel: impl Fn(u32, u32) -> Option<u32>,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    let mut colors = BTreeMap::new();
    for y in 0..height {
        for x in 0..width {
            let value = pixel(x, y).unwrap_or(0);
            *colors.entry(value).or_insert(0usize) += 1;
            let [b, g, r, _] = value.to_le_bytes();
            pixels.extend([r, g, b]);
        }
    }
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{width} {height}\n255\n")?;
    file.write_all(&pixels)?;
    let mut top: Vec<_> = colors.into_iter().collect();
    top.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    println!(
        "画面 {width}x{height} 颜色 {} 主色 {:?}",
        top.len(),
        &top[..top.len().min(5)]
    );
    Ok(())
}
