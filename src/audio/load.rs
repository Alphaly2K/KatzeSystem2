use super::{SoundState, Source, playback::Playback};
use crate::kcs::ResourceServices;
use crate::resource::ResourceMode;
use crate::{Error, Result, read};

impl SoundState {
    pub(super) fn load(
        &mut self,
        record: &[u8],
        services: Option<&mut dyn ResourceServices>,
        output: &mut u32,
        code: u32,
    ) -> Result<()> {
        let grouped = code >= 206;
        let whole = code & 1 != 0;
        let logical = if grouped {
            let groups = self
                .groups
                .as_ref()
                .ok_or_else(|| Error::new(0, "声音任务尚未配置组范围"))?;
            let id = read::u32_at(record, if code <= 207 { 4 } else { 8 })?;
            *output = u32::MAX;
            let group = if code <= 207 {
                groups.get(&id)
            } else {
                groups.values().find(|group| group.id == id)
            };
            let Some(group) = group else {
                return Ok(());
            };
            self.select_channel(group.range)?
        } else {
            let logical = read::u32_at(record, 0)?;
            if code == 181 && logical as i32 >= 64 {
                return Ok(());
            }
            logical
        };
        let record = read::range(record, 0, 0x120)?;
        let options = u32::from_le_bytes(record[0x114..0x118].try_into().unwrap());
        let mode = i32::from_le_bytes(record[0x118..0x11c].try_into().unwrap());
        let duration = i32::from_le_bytes(record[0x11c..0x120].try_into().unwrap());
        let offset = if mode == 0 { 0 } else { 32 };
        let physical = if code == 181 {
            Some(logical.wrapping_add(offset))
        } else {
            logical.checked_add(offset)
        }
        .filter(|&index| index < 64)
        .ok_or_else(|| Error::new(0, "声音装载通道或备用通道超出范围"))?;
        let name = &record[12..272];
        let length = name
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| Error::new(12, "声音装载名称缺少零结尾"))?;
        let name = &name[..length];
        let services = services.ok_or_else(|| Error::new(0, "宿主未提供声音资源服务"))?;
        let sound = &mut self.channels[physical as usize];
        sound.file.clear();
        sound.source = None;
        sound.playback = Playback::default();
        sound.resampler = super::mix::Resampler::default();
        self.output_replace(physical as usize);
        let sound = &mut self.channels[physical as usize];
        if let Some(pcm) = load_source(services, name, whole)? {
            let playback = Playback::new(&pcm, whole, options)?;
            sound.file.extend_from_slice(name);
            sound.pcm_volume = 100;
            sound.pcm_pan = 0;
            sound.distance_range = [100.0; 2];
            sound.position = [0.0; 3];
            sound.velocity = [0.0; 3];
            sound.doppler_factor = 1.0;
            sound.source = Some(pcm);
            sound.playback = playback;
        }
        if grouped && sound.source.is_none() {
            return Ok(());
        }
        *output = if grouped {
            physical
        } else {
            u32::from(sound.source.is_some())
        };
        sound.clock.elapsed = 0;
        sound.ramps.active = 0;
        // 209 保留分配后的循环缓存，包括全忙回收只清起点的结果。
        if code != 209 {
            sound.loop_range = [0, -1];
            if !whole && u32::from_le_bytes(record[0x110..0x114].try_into().unwrap()) != 0 {
                if let Some(point) = self.loops.find(name) {
                    sound.loop_range = [point.start, point.end];
                }
            }
        }
        sound.parameters = [mode, duration];
        sound.load_options = options;
        sound.started_tick = u32::MAX;
        sound.linked = None;
        sound.grouped = grouped;
        sound.allocation_flags = [0; 2];
        sound.deferred_control = None;
        if mode != 0 && (grouped || whole) {
            sound.linked = Some(physical);
        } else if mode != 0 {
            self.channels[logical as usize].linked = Some(physical);
        }
        self.output_replace(physical as usize);
        load_metadata(services, name, &mut self.channels[physical as usize].tags)
    }
}

fn load_source(
    services: &mut dyn ResourceServices,
    file: &[u8],
    whole: bool,
) -> Result<Option<Source>> {
    let Some(source) = services.open(file, ResourceMode::LooseFirst)? else {
        return Ok(None);
    };
    if source.is_empty()? {
        return Ok(None);
    }
    Source::from_vorbis(source, whole).map(Some)
}

fn load_metadata(
    services: &mut dyn ResourceServices,
    file: &[u8],
    tags: &mut std::collections::BTreeMap<Vec<u8>, u32>,
) -> Result<()> {
    tags.clear();
    let basename = file
        .rsplit(|byte| matches!(byte, b'/' | b'\\'))
        .next()
        .unwrap();
    let stem = without_extension(basename);
    for (category, extension) in [
        (b"tag".as_slice(), None),
        (b"sound".as_slice(), Some(b"plv".as_slice())),
        (b"sound".as_slice(), Some(b"lpb".as_slice())),
        (b"sound".as_slice(), Some(b"lv".as_slice())),
    ] {
        let mut name = services.resolve(category, stem)?;
        name.truncate(
            name.iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len()),
        );
        if let Some(extension) = extension {
            name.truncate(without_extension(&name).len());
            name.push(b'.');
            name.extend(extension);
        }
        if name.len() >= 1024 {
            return Err(Error::new(0, "声音附加资源名称超过缓冲区容量"));
        }
        if let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? {
            if category == b"tag" {
                *tags = super::tags::parse(&bytes)?;
            } else if !bytes.is_empty() {
                return Err(Error::new(0, "声音附加资源内容尚未接入"));
            }
        }
    }
    Ok(())
}

fn without_extension(name: &[u8]) -> &[u8] {
    let dot = name
        .iter()
        .rposition(|byte| matches!(byte, b'.' | b'/' | b'\\'))
        .filter(|&position| name[position] == b'.');
    &name[..dot.unwrap_or(name.len())]
}
