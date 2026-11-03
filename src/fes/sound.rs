use super::ObjectAttributes;
use crate::{Error, Result};

/// SOUND 的声明属性及运动状态；文件请求由媒体更新阶段消费。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundState {
    pub file: Vec<u8>,
    pub channel: i32,
    pub sound_type: i32,
    pub object_type: i32,
    pub group: Vec<u8>,
    pub volume: i32,
    pub pan: i32,
    pub(crate) dirty: bool,
    pub(crate) load_requested: bool,
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            file: Vec::new(),
            channel: -1,
            sound_type: 0,
            object_type: 0,
            group: Vec::new(),
            volume: 100,
            pan: 0,
            dirty: false,
            load_requested: false,
        }
    }
}

impl SoundState {
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn needs_load(&self) -> bool {
        self.load_requested
    }

    /// FILE 重复写入仍请求装载；名称保留原始字节，不执行图像名称转换。
    pub fn apply(&mut self, attributes: &ObjectAttributes) -> Result<bool> {
        if let Some(file) = &attributes.file {
            self.file = name(file, 260)?.to_vec();
            self.load_requested = true;
        }
        let mut dirty = false;
        if let Some(sound_type) = attributes.sound_type {
            dirty |= self.sound_type != sound_type;
            self.sound_type = sound_type;
        }
        if let Some(channel) = attributes.channel {
            dirty |= self.channel != channel;
            self.channel = channel;
        }
        self.dirty |= dirty;
        if let Some(object_type) = attributes.object_type {
            self.object_type = object_type;
        }
        if let Some(group) = &attributes.group {
            self.group = name(group, 32)?.to_vec();
        }
        Ok(dirty)
    }

    pub fn motion_properties(&self) -> [u32; 36] {
        let mut properties = [0; 36];
        properties[6] = (self.pan as f32).to_bits();
        properties[28] = (self.volume as u32) << 24;
        properties
    }

    pub fn apply_motion_properties(&mut self, properties: &[u32; 36]) -> bool {
        let pan = f32::from_bits(properties[6]);
        let pan = if (-2147483648.0..2147483648.0).contains(&pan) {
            pan as i32
        } else {
            i32::MIN
        };
        let volume = (properties[28] >> 24) as i32;
        let dirty = self.pan != pan || self.volume != volume;
        self.pan = pan;
        self.volume = volume;
        self.dirty |= dirty;
        dirty
    }
}

fn name(bytes: &[u8], capacity: usize) -> Result<&[u8]> {
    let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
    if bytes.len() >= capacity {
        return Err(Error::new(0, "FES SOUND 名称超过存储上限"));
    }
    Ok(bytes)
}
