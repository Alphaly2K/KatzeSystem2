use super::{SystemSettings, put, word};
use crate::{Error, Result, resource::XmlDocument};

impl SystemSettings {
    pub(super) fn apply_voice_definitions(&mut self, xml: &XmlDocument) -> Result<()> {
        for index in 0..48 {
            let base = 20 + index * 116;
            for (field, offset, capacity) in [("voice", 4, 32), ("head", 40, 40), ("int", 84, 32)] {
                if let Some(value) = xml.text(format!("VOICE/v{index:02}/{field}").as_bytes())? {
                    if value.len() >= capacity {
                        return Err(Error::new(0, "声音定义字段超过基线容量"));
                    }
                    let target = &mut self.sound[base + offset..base + offset + capacity];
                    target[..value.len()].copy_from_slice(value);
                    target[value.len()] = 0;
                }
            }
            put(&mut self.sound, base, 1);
            for (field, offset) in [("mastervol", 36), ("group", 80)] {
                let value = xml.integer(
                    format!("VOICE/v{index:02}/{field}").as_bytes(),
                    word(&self.sound, base + offset),
                )?;
                put(&mut self.sound, base + offset, value);
            }
            lowercase(&mut self.sound[base + 84..base + 116]);
        }
        Ok(())
    }

    pub(crate) fn voice_file_index(&self, name: &[u8]) -> Result<Option<usize>> {
        let name = terminated(name);
        if name.len() >= 260 {
            return Err(Error::new(0, "声音名称超过查询缓冲区容量"));
        }
        let mut position = 0;
        let mut separator = 0;
        while position < name.len() {
            let length = character_length(&name[position..]);
            if length == 1 && matches!(name[position], b'/' | b'\\') {
                separator = position;
            }
            position += length;
        }
        if separator != 0 {
            let mut directory = &name[..separator];
            if let Some(last) = directory
                .iter()
                .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
            {
                if directory[last] == b'.' {
                    directory = &directory[..last];
                }
            }
            if let Some(index) = (0..48).find(|&index| {
                terminated(&self.sound[20 + index * 116 + 84..20 + (index + 1) * 116])
                    .eq_ignore_ascii_case(directory)
            }) {
                return Ok(Some(index));
            }
        }
        let mut name = name.to_vec();
        lowercase(&mut name);
        Ok((0..48).find(|&index| {
            let start = 20 + index * 116 + 40;
            let prefix = terminated(&self.sound[start..start + 40]);
            !prefix.is_empty() && name.starts_with(prefix)
        }))
    }

    pub(crate) fn voice_file_play_enabled(&self, name: &[u8]) -> Result<u32> {
        self.voice_file_field(name, 0)
    }

    pub(crate) fn voice_file_volume(&self, name: &[u8]) -> Result<u32> {
        self.voice_file_field(name, 36)
    }

    fn voice_file_field(&self, name: &[u8], offset: usize) -> Result<u32> {
        Ok(self
            .voice_file_index(name)?
            .map_or(0, |index| word(&self.sound, 20 + index * 116 + offset)))
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}

fn character_length(bytes: &[u8]) -> usize {
    if matches!(bytes[0], 0x81..=0x9f | 0xe0..=0xef)
        && bytes
            .get(1)
            .is_some_and(|byte| matches!(byte, 0x40..=0x7e | 0x80..=0xfc))
    {
        2
    } else {
        1
    }
}

fn lowercase(bytes: &mut [u8]) {
    let mut position = 0;
    while position < bytes.len() && bytes[position] != 0 {
        let length = character_length(&bytes[position..]);
        if length == 1 {
            bytes[position].make_ascii_lowercase();
        }
        position += length;
    }
}
