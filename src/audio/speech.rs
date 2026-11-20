use super::{SoundState, Source, playback::Playback};
use crate::host::{ResourceServices, SpeechSynthesizer};
use crate::{Error, Result, read};

impl SoundState {
    pub(super) fn load_speech(
        &mut self,
        record: &[u8],
        code: u32,
        synth: &mut dyn SpeechSynthesizer,
    ) -> Result<u32> {
        let grouped = code != 225;
        let logical = if grouped {
            let key = read::u32_at(record, if code == 226 { 4 } else { 8 })?;
            let range = if code == 226 {
                self.group_range(key)
            } else {
                self.groups.as_ref().and_then(|groups| {
                    groups
                        .values()
                        .find(|group| group.id == key)
                        .map(|group| group.range)
                })
            };
            let Some(range) = range else {
                return Ok(u32::MAX);
            };
            self.select_channel(range)?
        } else {
            read::u32_at(record, 0)?
        };
        let options = read::u32_at(record, 0x114)?;
        let mode = read::u32_at(record, 0x118)? as i32;
        let duration = read::u32_at(record, 0x11c)? as i32;
        let physical = speech_channel(logical, mode)?;
        let text = read::zstr(record, 0x120)?;
        if text.len() >= 2048 {
            return Err(Error::new(0, "语音合成文字超过格式缓冲区容量"));
        }
        let source = synth
            .synthesize(text)?
            .filter(|pcm| pcm.frames() != 0)
            .map(Source::from);
        let sound = &mut self.channels[physical];
        let playback = if let Some(source) = &source {
            let mut playback = Playback::new(source, false, options)?;
            playback.prepare_signal(source, sound.signal_enabled)?;
            sound.pcm_volume = 100;
            sound.pcm_pan = 0;
            sound.distance_range = [100.; 2];
            sound.position = [0.; 3];
            sound.velocity = [0.; 3];
            sound.doppler_factor = 1.;
            playback
        } else {
            Playback::default()
        };
        let loaded = source.is_some();
        sound.file.clear();
        sound.source = source;
        sound.playback = playback;
        sound.resampler = Default::default();
        if !grouped || loaded {
            sound.clock.elapsed = 0;
            sound.loop_range = [0, -1];
            sound.ramps.active = 0;
            sound.parameters = [mode, duration];
            sound.load_options = options;
            sound.started_tick = u32::MAX;
            sound.linked = (grouped && mode != 0).then_some(physical as u32);
            sound.grouped = grouped;
            sound.allocation_flags = [0; 2];
            sound.deferred_control = None;
        }
        if !grouped && mode != 0 {
            self.channels[logical as usize].linked = Some(physical as u32);
        }
        self.output_replace(physical);
        Ok(if !grouped {
            u32::from(loaded)
        } else if loaded {
            physical as u32
        } else {
            u32::MAX
        })
    }

    pub(super) fn load_speech_tags(
        &mut self,
        record: &[u8],
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<()> {
        let physical = speech_channel(
            read::u32_at(record, 0)?,
            read::u32_at(record, 0x118)? as i32,
        )?;
        let tags = &mut self.channels[physical].tags;
        tags.clear();
        if let Some(services) = services {
            let name = read::zstr(read::range(record, 12, 260)?, 0)?;
            let stem = super::load::metadata_stem(name);
            super::load::load_tags(services, stem, tags)?;
        }
        Ok(())
    }
}

fn speech_channel(logical: u32, mode: i32) -> Result<usize> {
    logical
        .checked_add(if mode == 0 { 0 } else { 32 })
        .filter(|&index| index < 64)
        .map(|index| index as usize)
        .ok_or_else(|| Error::new(0, "语音合成通道超出范围"))
}
