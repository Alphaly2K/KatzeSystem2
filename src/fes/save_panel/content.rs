use super::{PanelCell, SavePanelState};
use crate::{Error, Result, graphics::Image};

/// 面板的已应用内容；记录中的图像与文字地址固定为零，由独立字段持有。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelContent {
    data: [u8; 188],
    text: Vec<u8>,
    image: Option<Image>,
}

impl Default for PanelContent {
    fn default() -> Self {
        Self {
            data: [0; 188],
            text: Vec::new(),
            image: None,
        }
    }
}

impl PanelContent {
    pub fn record(&self) -> &[u8; 188] {
        &self.data
    }

    pub fn kind(&self) -> u32 {
        self.word(0)
    }

    pub fn text(&self) -> &[u8] {
        &self.text
    }

    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    pub fn number(&self) -> i32 {
        self.word(0x40) as i32
    }

    pub(crate) fn set_number(&mut self, value: i32) {
        self.put(0x40, value as u32);
    }

    pub(crate) fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn put(&mut self, offset: usize, value: u32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn reference(&mut self, definition: &PanelCell) {
        let name = definition.data[0x20..0x40]
            .split(|&b| b == 0)
            .next()
            .unwrap();
        self.data[4..4 + name.len()].copy_from_slice(name);
        self.data[4 + name.len()] = 0;
        self.put(0x24, definition.word(0x40));
    }
}

impl SavePanelState {
    /// 按编号应用全部定义，保持未被本次来源更新的内容及启用编号。
    pub fn apply_data(&mut self) -> Result<()> {
        for (&id, definition) in &self.cells {
            match definition.kind() {
                0 => self.contents.entry(id).or_default().put(0, 0),
                1 | 7 => {
                    if definition.kind() == 1 && definition.source_kind() != 1 {
                        let content = self.contents.entry(id).or_default();
                        content.put(0, 1);
                        content.put(0x28, 0);
                        content.put(0x2c, 0);
                        content.image = None;
                        continue;
                    }
                    if definition.kind() == 7 && definition.source_kind() != 10 {
                        continue;
                    }
                    let Some(record) = self
                        .data
                        .as_ref()
                        .map(|data| data.record(definition.word(0x10)))
                        .transpose()?
                        .flatten()
                    else {
                        continue;
                    };
                    if let Some(image) = decode_image(record)? {
                        let content = self.contents.entry(id).or_default();
                        content.put(0, definition.kind());
                        content.put(0x28, 0);
                        content.put(0x2c, 1);
                        content.image = Some(image);
                    }
                    if let (7, Some(content)) = (definition.kind(), self.contents.get_mut(&id)) {
                        content.data[0xa4..].copy_from_slice(&definition.data[0xa4..]);
                    }
                }
                2 => {
                    let content = self.contents.entry(id).or_default();
                    content.put(0, 2);
                    content.reference(definition);
                }
                kind @ 3..=6 => {
                    let Some((value, text)) = self.cell_value(definition)? else {
                        continue;
                    };
                    let content = self.contents.entry(id).or_default();
                    if kind == 3 || kind == 4 {
                        content.put(0, 3);
                        content.text = text;
                        for (target, source) in [
                            (0x34, 0x14),
                            (0x38, 0x18),
                            (0x3c, 0x1c),
                            (0x4c, 0x48),
                            (0x50, 0x4c),
                            (0x54, 0x50),
                            (0x5c, 0x58),
                            (0x60, 0x5c),
                        ] {
                            content.put(target, definition.word(source));
                        }
                        let face = definition.data[0x64..].split(|&b| b == 0).next().unwrap();
                        content.data[0x64..0x64 + face.len()].copy_from_slice(face);
                        content.data[0x64 + face.len()] = 0;
                    } else {
                        content.put(0, kind);
                        content.set_number(value);
                        if kind == 5 {
                            content.reference(definition);
                        } else {
                            content.put(0x28, definition.word(0x44));
                            content.image = None;
                        }
                        content.put(0x44, definition.word(0xc));
                        content.put(0x48, definition.word(0x54));
                        content.put(0x5c, 1);
                        content.put(0x60, 0);
                    }
                }
                _ => (),
            }
        }
        self.image.base.updates.request_rebuild();
        Ok(())
    }

    fn cell_value(&self, definition: &PanelCell) -> Result<Option<(i32, Vec<u8>)>> {
        let value = match definition.source_kind() {
            0 => self
                .flags
                .get(definition.word(8) as usize)
                .copied()
                .unwrap_or(0),
            2 => {
                let Some(record) = self
                    .data
                    .as_ref()
                    .map(|data| data.record(definition.word(0x10)))
                    .transpose()?
                    .flatten()
                else {
                    return Ok(None);
                };
                if record.len() >= 256 {
                    return Ok(None);
                }
                if record.is_empty() {
                    return Ok(Some((0, Vec::new())));
                }
                let end = record
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or_else(|| Error::new(0, "面板文字记录缺少结束字节"))?;
                return Ok(Some((0, record[..end].to_vec())));
            }
            source @ 3..=8 => self.date[(source - 3) as usize] as i32,
            9 => definition.word(0x60) as i32,
            _ => return Ok(None),
        };
        Ok(Some((value, value.to_string().into_bytes())))
    }
}

fn decode_image(bytes: &[u8]) -> Result<Option<Image>> {
    if bytes.starts_with(b"HG-3") {
        return crate::hg3::decode_frame(bytes, 0)?
            .map(Image::from_frame)
            .transpose();
    }
    if bytes.starts_with(b"HG-2")
        || bytes.starts_with(b"BM")
        || bytes.starts_with(b"\xff\xd8")
        || bytes.starts_with(b"\x89PNG")
    {
        return Err(Error::new(0, "面板存档图像编码尚未接入"));
    }
    Ok(None)
}
