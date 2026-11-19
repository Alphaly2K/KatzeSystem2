//! 从宿主提供的字体数据选择、度量并生成灰度字形。

use super::{Font, FontRequest, FontServices, GlyphMask};
use crate::{
    Error, Result,
    io::{ReadSource, SharedSource},
    read,
};
use std::sync::Arc;

mod catalog;
use catalog::{Face, Source, charset_bit};

type ResourceLoader = dyn FnMut(&[u8]) -> Result<Option<SharedSource>> + Send + Sync;

/// 私有字体优先于同名的宿主回退字体；字体轮廓仅在选中后装载。
/// 空名称选择默认字体并允许缺字回退，显式名称保持指定字体。
#[derive(Default)]
pub struct Fonts {
    faces: Vec<Face>,
    local_count: usize,
    resource_loader: Option<Box<ResourceLoader>>,
    selections: Vec<(FontRequest, usize)>,
}

impl Fonts {
    /// 建立空字体目录。宿主可显式注册字体及回退来源。
    pub fn new() -> Self {
        Self::default()
    }

    /// 为脚本私有字体注册提供读取源。回调接收原始 CP932 名称；None 表示资源缺失。
    pub fn set_resource_loader(
        &mut self,
        loader: impl FnMut(&[u8]) -> Result<Option<SharedSource>> + Send + Sync + 'static,
    ) {
        self.resource_loader = Some(Box::new(loader));
    }

    /// 原子登记字体或集合的全部成员，仅此服务可见；失败保留已有字体。
    pub fn add_data(&mut self, bytes: &[u8]) -> Result<usize> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字体数据超过存储上限"));
        }
        let data: Arc<[u8]> = bytes.into();
        let faces = catalog::parse(&data, Source::Data(data.clone()))?;
        let count = self.insert_faces(self.local_count, faces)?;
        self.local_count += count;
        Ok(count)
    }

    /// 登记一个回退字体或集合。打开一次提取元数据，选中时重新打开以装载轮廓。
    /// 每次打开须返回相同的字体数据。多个来源按注册顺序参与选择与枚举。
    pub fn add_fallback_source(
        &mut self,
        open: impl Fn() -> Result<SharedSource> + Send + Sync + 'static,
    ) -> Result<usize> {
        let bytes = read_font(open()?.as_ref())?;
        let faces = catalog::parse(&bytes, Source::Deferred(Arc::new(open)))?;
        self.insert_faces(self.faces.len(), faces)
    }

    fn insert_faces(&mut self, at: usize, faces: Vec<Face>) -> Result<usize> {
        if self.faces.len().saturating_add(faces.len()) > read::MAX_ITEMS {
            return Err(Error::new(0, "字体数量超过存储上限"));
        }
        let count = faces.len();
        self.faces.splice(at..at, faces);
        self.selections.clear();
        Ok(count)
    }

    fn select(&mut self, request: &FontRequest, size: u32) -> Result<usize> {
        check_request(request, size)?;
        if let Some((_, index)) = self.selections.iter().find(|(key, _)| key == request) {
            return Ok(*index);
        }
        let name = String::from_utf16(terminated(&request.name))
            .unwrap()
            .to_lowercase();
        let mut index = self.named_face(&name, request, self.local_count);
        if index.is_none() || name.is_empty() {
            index = if name.is_empty() {
                self.default_face(request)
            } else {
                self.named_face(&name, request, self.faces.len())
            };
        }
        let index = index.ok_or_else(|| {
            Error::new(
                0,
                if name.is_empty() {
                    "没有可用的默认字体"
                } else {
                    "命名字体不可用"
                },
            )
        })?;
        if self.selections.len() >= 64 {
            self.selections.remove(0);
        }
        self.selections.push((request.clone(), index));
        Ok(index)
    }

    fn named_face(&self, name: &str, request: &FontRequest, limit: usize) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        (0..limit)
            .filter(|&index| self.faces[index].names.iter().any(|alias| alias == name))
            .min_by_key(|&index| self.faces[index].style_distance(request))
    }

    fn default_face(&self, request: &FontRequest) -> Option<usize> {
        let preferences: &[&str] = if request.glyph_parameter as u8 == 128 {
            &[
                "hiragino sans",
                "noto sans cjk jp",
                "noto sans jp",
                "yu gothic",
                "meiryo",
            ]
        } else {
            &[
                "arial",
                "liberation sans",
                "dejavu sans",
                "noto sans",
                "hiragino sans",
            ]
        };
        for name in preferences {
            if let Some(index) = self
                .named_face(name, request, self.faces.len())
                .filter(|&index| !self.faces[index].last_resort)
            {
                return Some(index);
            }
        }
        (0..self.faces.len())
            .filter(|&index| {
                !self.faces[index].last_resort
                    && self.faces[index].supports_charset(request.glyph_parameter as u8 as u32)
            })
            .min_by_key(|&index| self.faces[index].style_distance(request))
            .or_else(|| self.faces.iter().position(|face| !face.last_resort))
    }

    fn glyph_face(&self, primary: usize, request: &FontRequest, character: char) -> Result<usize> {
        if self.faces[primary].has_character(character) {
            return Ok(primary);
        }
        if terminated(&request.name).is_empty() {
            if let Some(index) = (0..self.faces.len())
                .filter(|&index| {
                    !self.faces[index].last_resort && self.faces[index].has_character(character)
                })
                .min_by_key(|&index| self.faces[index].style_distance(request))
            {
                return Ok(index);
            }
            return Ok(primary);
        }
        Err(Error::new(
            0,
            format!("字体缺少字符 U+{:04X}", character as u32),
        ))
    }

    fn font(&mut self, index: usize) -> Result<&Font> {
        let face = &mut self.faces[index];
        if face.font.is_none() {
            face.font = Some(match &face.source {
                Source::Data(bytes) => Font::from_bytes(bytes, face.index)?,
                Source::Deferred(open) => {
                    Font::from_bytes(&read_font(open()?.as_ref())?, face.index)?
                }
            });
        }
        Ok(face.font.as_ref().unwrap())
    }
}

impl FontServices for Fonts {
    fn add_resource(&mut self, name: &[u8]) -> Result<bool> {
        let loader = self
            .resource_loader
            .as_mut()
            .ok_or_else(|| Error::new(0, "字体服务未提供字体资源注册"))?;
        let Some(source) = loader(name)? else {
            return Ok(false);
        };
        Ok(self.add_data(&read_font(source.as_ref())?).is_ok())
    }

    fn enumerate(&mut self, character_set: u32, variable_pitch: u32) -> Result<Vec<Vec<u16>>> {
        Ok(self
            .faces
            .iter()
            .filter(|face| {
                (variable_pitch != 0 || face.monospaced) && face.supports_charset(character_set)
            })
            .filter_map(|face| {
                let name = &face.family;
                (!name.is_empty() && name.first() != Some(&(b'@' as u16)))
                    .then(|| name.iter().take(63).copied().collect())
            })
            .collect())
    }

    fn validate(&mut self, request: &FontRequest) -> Result<()> {
        let index = self.select(request, 16)?;
        self.font(index)?;
        Ok(())
    }

    fn measure(&mut self, request: &FontRequest, character: char, size: u32) -> Result<i32> {
        let primary = self.select(request, size)?;
        if size == 0 {
            return Ok(0);
        }
        let index = self.glyph_face(primary, request, character)?;
        let (bold, _) = self.faces[index].synthetic_style(request, size);
        let measured = styled_advance(
            self.font(index)?.measure_or_missing(character, size)?,
            character,
            bold,
        )?;
        request.character_advance(character, size, measured)
    }

    fn rasterize(
        &mut self,
        request: &FontRequest,
        character: char,
        size: u32,
    ) -> Result<GlyphMask> {
        let primary = self.select(request, size)?;
        if size == 0 {
            return GlyphMask::new([0; 2], [0; 2], 0, Vec::new());
        }
        let index = self.glyph_face(primary, request, character)?;
        let (bold, italic) = self.faces[index].synthetic_style(request, size);
        let baseline = self.font(primary)?.baseline(size);
        let font = self.font(index)?;
        let measured = styled_advance(font.measure_or_missing(character, size)?, character, bold)?;
        let advance = request.character_advance(character, size, measured)?;
        if matches!(character, ' ' | '\u{3000}') {
            return GlyphMask::new([0; 2], [0; 2], advance, Vec::new());
        }
        let glyph = font.rasterize_or_missing(character, size)?;
        let mut offset = glyph.offset();
        offset[1] = baseline
            .checked_sub(font.baseline(size))
            .and_then(|shift| offset[1].checked_add(shift))
            .ok_or_else(|| Error::new(0, "字形位置超出支持范围"))?;
        let (dimensions, coverage) = styled_coverage(&glyph, bold, italic)?;
        if request.measure_parameter != 0 && advance > measured {
            offset[0] = ((advance - dimensions[0] as i32) / 2 + 1).max(0);
        }
        GlyphMask::new(dimensions, offset, advance, coverage)
    }
}

fn check_request(request: &FontRequest, size: u32) -> Result<()> {
    if request.backend != 0 {
        return Err(Error::new(0, "字体服务不支持该字体后端"));
    }
    if !matches!(request.glyph_parameter as u8, 1)
        && charset_bit(request.glyph_parameter as u8 as u32).is_none()
    {
        return Err(Error::new(0, "字体服务不支持该字符集"));
    }
    if size > i32::MAX as u32 {
        return Err(Error::new(0, "字号超过字形服务支持范围"));
    }
    let name = terminated(&request.name);
    if name.len() > 63 || String::from_utf16(name).is_err() || name.first() == Some(&(b'@' as u16))
    {
        return Err(Error::new(0, "字体名称不是受支持的水平字体名"));
    }
    Ok(())
}

fn terminated(name: &[u16]) -> &[u16] {
    &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]
}

fn read_font(source: &dyn ReadSource) -> Result<Vec<u8>> {
    let length = source.len()?;
    if length > read::MAX_BYTES as u64 {
        return Err(Error::new(0, "字体数据超过存储上限"));
    }
    let mut bytes = vec![0; length as usize];
    source.read_exact_at(0, &mut bytes)?;
    Ok(bytes)
}

fn styled_advance(measured: i32, character: char, bold: u32) -> Result<i32> {
    measured
        .checked_add(if matches!(character, ' ' | '\u{3000}') {
            0
        } else {
            bold as i32
        })
        .ok_or_else(|| Error::new(0, "字形度量超出支持范围"))
}

fn styled_coverage(glyph: &GlyphMask, bold: u32, italic: bool) -> Result<([u32; 2], Vec<u8>)> {
    let [width, height] = glyph.size();
    if width == 0 || height == 0 || (bold == 0 && !italic) {
        return Ok((glyph.size(), glyph.coverage().to_vec()));
    }
    // 缺少真实样式时以灰度扩张和逐行倾斜合成，字宽与绘制共用同一加粗量。
    let shear = if italic { (height - 1).div_ceil(5) } else { 0 };
    let output_width = width
        .checked_add(bold)
        .and_then(|value| value.checked_add(shear))
        .filter(|&value| value <= i32::MAX as u32)
        .ok_or_else(|| Error::new(0, "字形尺寸超出支持范围"))?;
    let length = (output_width as usize)
        .checked_mul(height as usize)
        .filter(|&length| length <= read::MAX_BYTES / 4)
        .ok_or_else(|| Error::new(0, "字形像素超过存储上限"))?;
    let mut coverage = vec![0; length];
    for y in 0..height {
        let shift = if italic {
            (height - 1 - y).div_ceil(5)
        } else {
            0
        };
        for x in 0..width {
            let value = glyph.coverage()[(y * width + x) as usize];
            for extra in 0..=bold {
                let destination = &mut coverage[(y * output_width + x + shift + extra) as usize];
                *destination = (*destination).max(value);
            }
        }
    }
    Ok(([output_width, height], coverage))
}
