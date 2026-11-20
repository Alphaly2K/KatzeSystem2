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
