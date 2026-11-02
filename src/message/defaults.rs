use super::TextEncoding;
use crate::{Error, Result};

/// 全局消息设置；字体名保留 UTF-16，附加字形参数未覆盖时使用一。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageDefaults {
    pub font_name: Vec<u16>,
    pub bold: bool,
    pub shadow: u32,
    pub outline: u32,
    pub glyph_parameter: Option<u32>,
    pub reveal: bool,
}

impl MessageDefaults {
    /// 生成对象 455／476 使用的完整默认配置，包含大小标签。
    pub fn configuration(&self, encoding: TextEncoding) -> Result<Vec<u8>> {
        let (length, extra) = match encoding {
            TextEncoding::Cp932 => (0xe8, 0),
            TextEncoding::Unicode => (0x12c, 0x40),
        };
        let name = &self.font_name[..self
            .font_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.font_name.len())];
        if name.len() >= (0x40 + extra) / 2 {
            return Err(Error::new(0, "消息默认字体名超过配置容量"));
        }
        let mut bytes = vec![0; length];
        let mut write = |offset, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        for (offset, value) in [
            (0, length as u32),
            (4, u32::from(self.reveal)),
            (8, 800),
            (12, 600),
            (0x10, u32::MAX),
            (0x14, 0xff00_0000),
            (0x18, u32::MAX),
            (0x60, 2),
            (0x64, 1),
            (0x6c, 1),
            (0xc4 + extra, if self.bold { 700 } else { 400 }),
            (0xc8 + extra, self.shadow),
            (0xcc + extra, self.outline),
            (0xd0 + extra, self.glyph_parameter.unwrap_or(1)),
            (0xd4 + extra, 60),
        ] {
            write(offset, value);
        }
        for index in 0..5 {
            write(0x1c + index * 4, 24);
            write(0x30 + index * 4, 10);
        }
        for (index, &unit) in name.iter().enumerate() {
            bytes[0x84 + index * 2..0x86 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        Ok(bytes)
    }
}
