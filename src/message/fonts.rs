//! 字体发现、选择、度量与灰度栅格化。

use super::{Font, FontRequest, FontServices, GlyphMask};
use crate::{Error, Result, read};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

mod catalog;
use catalog::{Face, Source, charset_bit};

/// 私有字体优先于系统同名字体；字体轮廓仅在选中后装载。
/// 空名称选择默认字体并允许缺字回退，显式名称保持指定字体。
pub struct Fonts {
    root: PathBuf,
    faces: Vec<Face>,
    local_count: usize,
    system_pending: bool,
    selections: Vec<(FontRequest, usize)>,
}

impl Fonts {
    /// 按需发现系统字体；构造时不扫描目录或装载字体轮廓。
    pub fn new(resource_root: impl Into<PathBuf>) -> Self {
        Self {
            system_pending: true,
            ..Self::without_system_fonts(resource_root)
        }
    }

    /// 仅使用显式注册字体，适用于确定性运行及没有系统字体的宿主。
    pub fn without_system_fonts(resource_root: impl Into<PathBuf>) -> Self {
        Self {
            root: resource_root.into(),
            faces: Vec::new(),
            local_count: 0,
            system_pending: false,
            selections: Vec::new(),
        }
    }

    /// 原子登记字体或集合的全部成员，仅此服务可见；失败保留已有字体。
    pub fn add_data(&mut self, bytes: &[u8]) -> Result<usize> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字体数据超过存储上限"));
        }
        let data: Arc<[u8]> = bytes.into();
        let faces = catalog::parse(&data, Source::Data(data.clone()))?;
        if self.faces.len().saturating_add(faces.len()) > read::MAX_ITEMS {
            return Err(Error::new(0, "字体数量超过存储上限"));
        }
        let count = faces.len();
        self.faces.splice(self.local_count..self.local_count, faces);
        self.local_count += count;
        self.selections.clear();
        Ok(count)
    }

    /// 提前发现系统字体；可在进入帧循环前调用，后续选择复用同一目录。
    pub fn discover_system(&mut self) {
        if !std::mem::take(&mut self.system_pending) {
            return;
        }
        let timing = std::env::var_os("KATZE_TRACE_TIMING").map(|_| std::time::Instant::now());
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let files: BTreeSet<_> = database
            .faces()
            .filter_map(|face| {
                if let fontdb::Source::File(path) = &face.source {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();
        for path in files {
            let Ok(bytes) = read_file(&path) else {
                continue;
            };
            let Ok(faces) = catalog::parse(&bytes, Source::File(path)) else {
                continue;
            };
            if self.faces.len().saturating_add(faces.len()) > read::MAX_ITEMS {
                break;
            }
            self.faces.extend(faces);
        }
        if let Some(started) = timing {
            eprintln!(
                "[font-discovery] faces={} ms={:.2}",
                self.faces.len(),
                started.elapsed().as_secs_f64() * 1000.
            );
        }
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
            self.discover_system();
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
            let timing = std::env::var_os("KATZE_TRACE_TIMING").map(|_| std::time::Instant::now());
            let bytes = match &face.source {
                Source::Data(bytes) => bytes.clone(),
                Source::File(path) => read_file(path)?.into(),
            };
            face.font = Some(Font::from_bytes(&bytes, face.index)?);
            if let Some(started) = timing {
                eprintln!(
                    "[font-load] index={index} ms={:.2}",
                    started.elapsed().as_secs_f64() * 1000.
                );
            }
        }
        Ok(face.font.as_ref().unwrap())
    }
}

impl FontServices for Fonts {
    fn add_resource(&mut self, name: &[u8]) -> Result<bool> {
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(name);
        if invalid || name.contains('\0') {
            return Err(Error::new(0, "字体资源名称不是有效 CP932 路径"));
        }
        let path = self.root.join(name.replace('\\', "/"));
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(Error::new(0, "字体资源读取失败")),
        };
        let bytes = read_bytes(file)?;
        Ok(self.add_data(&bytes).is_ok())
    }

    fn enumerate(&mut self, character_set: u32, variable_pitch: u32) -> Result<Vec<Vec<u16>>> {
        self.discover_system();
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

fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|_| Error::new(0, "字体资源读取失败"))?;
    read_bytes(file)
}

fn read_bytes(file: std::fs::File) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(read::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new(0, "字体资源读取失败"))?;
    if bytes.len() > read::MAX_BYTES {
        return Err(Error::new(0, "字体数据超过存储上限"));
    }
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
