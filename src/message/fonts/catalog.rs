use super::{Font, FontRequest};
use crate::{Error, Result, read};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(super) enum Source {
    Data(Arc<[u8]>),
    File(PathBuf),
}

pub(super) struct Face {
    pub source: Source,
    pub index: u32,
    pub names: Vec<String>,
    pub family: Vec<u16>,
    pub monospaced: bool,
    pub last_resort: bool,
    weight: u16,
    italic: bool,
    codepages: Option<u32>,
    characters: Vec<(u32, u32)>,
    pub font: Option<Font>,
}

impl Face {
    pub fn style_distance(&self, request: &FontRequest) -> u32 {
        u32::from(self.italic != request.italic) * 1000
            + u32::from(self.weight).abs_diff(if request.weight == 700 { 700 } else { 400 })
    }

    pub fn synthetic_style(&self, request: &FontRequest, size: u32) -> (u32, bool) {
        (
            if request.weight == 700 && self.weight < 600 {
                size.div_ceil(32)
            } else {
                0
            },
            request.italic && !self.italic,
        )
    }

    pub fn has_character(&self, character: char) -> bool {
        let value = character as u32;
        self.characters
            .get(self.characters.partition_point(|&(_, end)| end < value))
            .is_some_and(|&(start, _)| start <= value)
    }

    pub fn supports_charset(&self, character_set: u32) -> bool {
        matches!(character_set, 1 | u32::MAX)
            || charset_bit(character_set)
                .zip(self.codepages)
                .is_some_and(|(bit, flags)| flags & (1 << bit) != 0)
    }
}

pub(super) fn parse(bytes: &[u8], source: Source) -> Result<Vec<Face>> {
    let count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    if count == 0 || count as usize > read::MAX_ITEMS {
        return Err(Error::new(0, "字体集合成员数量无效"));
    }
    let mut faces = Vec::new();
    for index in 0..count {
        let face = ttf_parser::Face::parse(bytes, index)
            .map_err(|_| Error::new(0, "字体或集合成员无法解析"))?;
        if [b"glyf", b"CFF "].iter().all(|tag| {
            face.raw_face()
                .table(ttf_parser::Tag::from_bytes(tag))
                .is_none()
        }) {
            return Err(Error::new(0, "字体没有受支持的轮廓"));
        }
        let mut aliases = Vec::new();
        let mut families = Vec::new();
        for name in face.names() {
            if !matches!(name.name_id, 1 | 4 | 6 | 16) {
                continue;
            }
            let text = if let Some(text) = name.to_string() {
                text
            } else if name.platform_id == ttf_parser::PlatformId::Macintosh && name.name.is_ascii()
            {
                String::from_utf8(name.name.to_vec()).unwrap()
            } else if name.platform_id == ttf_parser::PlatformId::Windows && name.encoding_id == 2 {
                let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(name.name);
                if invalid {
                    continue;
                }
                text.into_owned()
            } else {
                continue;
            };
            if text.is_empty() || text.contains('\0') {
                continue;
            }
            if matches!(name.name_id, 1 | 16) {
                families.push((
                    (name.name_id != 16, name.language_id != 0x409),
                    text.clone(),
                ));
            }
            let alias = text.to_lowercase();
            if !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
        families.sort_by_key(|(priority, _)| *priority);
        let family = families
            .first()
            .map(|(_, value)| value.as_str())
            .or_else(|| aliases.first().map(String::as_str))
            .ok_or_else(|| Error::new(0, "字体没有可用名称"))?
            .encode_utf16()
            .collect();
        let os2 = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"OS/2"))
            .unwrap_or_default();
        let codepages = if os2.len() >= 86 && u16::from_be_bytes([os2[0], os2[1]]) >= 1 {
            Some(u32::from_be_bytes(os2[78..82].try_into().unwrap()))
        } else {
            None
        };
        let mut characters = Vec::new();
        if let Some(cmap) = face.tables().cmap {
            for subtable in cmap.subtables {
                if subtable.is_unicode() {
                    subtable.codepoints(|code| {
                        if subtable.glyph_index(code).is_some_and(|glyph| glyph.0 != 0) {
                            characters.push(code);
                        }
                    });
                }
            }
        }
        characters.sort_unstable();
        characters.dedup();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for code in characters {
            if let Some((_, end)) = ranges.last_mut() {
                if end.checked_add(1) == Some(code) {
                    *end = code;
                    continue;
                }
            }
            ranges.push((code, code));
        }
        // head.flags 的 bit14 表示 cmap 只提供 Unicode 范围的通用占位字形。
        let head = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"head"))
            .unwrap();
        let last_resort = u16::from_be_bytes([head[16], head[17]]) & 0x4000 != 0;
        faces.push(Face {
            source: source.clone(),
            index,
            names: aliases,
            family,
            monospaced: face.is_monospaced(),
            last_resort,
            weight: face.weight().to_number(),
            italic: face.is_italic(),
            codepages,
            characters: ranges,
            font: None,
        });
    }
    Ok(faces)
}

pub(super) fn charset_bit(character_set: u32) -> Option<u32> {
    Some(match character_set {
        0 => 0,
        2 => 31,
        128 => 17,
        129 => 19,
        130 => 21,
        134 => 18,
        136 => 20,
        161 => 3,
        162 => 4,
        163 => 8,
        177 => 5,
        178 => 6,
        186 => 7,
        204 => 2,
        222 => 16,
        238 => 1,
        255 => 30,
        _ => return None,
    })
}
