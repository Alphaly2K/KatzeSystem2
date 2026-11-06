//! macOS CoreText 字体服务；不要求窗口或图形设备。

use super::{FontRequest, FontServices, GlyphMask};
use crate::{Error, Result, read};
use std::{path::PathBuf, ptr};

mod ffi;
use ffi::{Owned, Point, Size};

struct Face {
    font: Owned,
    names: Vec<Vec<u16>>,
}

struct CachedFont {
    request: FontRequest,
    size: u32,
    font: Owned,
}

/// 服务内字体资源优先于系统同名字体。空名称选择 macOS 用户默认字体。
/// 命名字体和缺字均不自动替换；CoreText 的度量及抗锯齿与 GDI 有平台差异。
pub struct MacFonts {
    root: PathBuf,
    faces: Vec<Face>,
    cache: Vec<CachedFont>,
}

impl MacFonts {
    pub fn new(resource_root: impl Into<PathBuf>) -> Self {
        Self {
            root: resource_root.into(),
            faces: Vec::new(),
            cache: Vec::new(),
        }
    }

    /// 装载字体或集合的全部成员，仅此服务可见；返回成功装载的字体数量。
    /// 失败时保留已有字体；同名资源按装载次序选择。
    pub fn add_data(&mut self, bytes: &[u8]) -> Result<usize> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字体数据超过存储上限"));
        }
        let data = unsafe {
            Owned::new(ffi::CFDataCreate(
                ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
            ))?
        };
        let descriptors =
            unsafe { Owned::new(ffi::CTFontManagerCreateFontDescriptorsFromData(data.get()))? };
        let count = unsafe { ffi::CFArrayGetCount(descriptors.get()) };
        let mut faces = Vec::new();
        for index in 0..count {
            let descriptor = unsafe { ffi::CFArrayGetValueAtIndex(descriptors.get(), index) };
            let font = unsafe {
                Owned::new(ffi::CTFontCreateWithFontDescriptor(
                    descriptor,
                    16.0,
                    ptr::null(),
                ))?
            };
            let names = font_names(&font)?;
            if names.is_empty() {
                return Err(Error::new(0, "字体没有可用名称"));
            }
            faces.push(Face { font, names });
        }
        if faces.is_empty() {
            return Err(Error::new(0, "没有可用的字体或集合成员"));
        }
        let count = faces.len();
        self.faces.extend(faces);
        self.cache.clear();
        Ok(count)
    }

    fn font(&mut self, request: &FontRequest, size: u32) -> Result<&Owned> {
        check_request(request, size)?;
        if let Some(index) = self
            .cache
            .iter()
            .position(|entry| entry.size == size && entry.request == *request)
        {
            return Ok(&self.cache[index].font);
        }
        let bold = request.weight == 700;
        let traits =
            if bold { ffi::BOLD } else { 0 } | if request.italic { ffi::ITALIC } else { 0 };
        let name = terminated(&request.name);
        let mut matching = self.faces.iter().filter(|face| {
            face.names
                .iter()
                .any(|candidate| same_name(candidate, name))
        });
        let local = matching
            .clone()
            .find(|face| style_traits(&face.font) == traits)
            .or_else(|| matching.next());
        let font = if name.is_empty() {
            unsafe {
                Owned::new(ffi::CTFontCreateUIFontForLanguage(
                    0,
                    f64::from(size.max(1)),
                    ptr::null(),
                ))?
            }
        } else if let Some(face) = local {
            unsafe {
                Owned::new(ffi::CTFontCreateCopyWithAttributes(
                    face.font.get(),
                    f64::from(size.max(1)),
                    ptr::null(),
                    ptr::null(),
                ))?
            }
        } else {
            let name_string = cf_string(name)?;
            let font = unsafe {
                Owned::new(ffi::CTFontCreateWithName(
                    name_string.get(),
                    f64::from(size.max(1)),
                    ptr::null(),
                ))?
            };
            if !font_names(&font)?
                .iter()
                .any(|candidate| same_name(candidate, name))
            {
                return Err(Error::new(0, "命名字体不可用"));
            }
            font
        };
        let font = if style_traits(&font) == traits {
            font
        } else {
            unsafe {
                Owned::new(ffi::CTFontCreateCopyWithSymbolicTraits(
                    font.get(),
                    f64::from(size.max(1)),
                    ptr::null(),
                    traits,
                    ffi::BOLD | ffi::ITALIC,
                ))
            }
            .map_err(|_| Error::new(0, "字体没有请求的字重或斜体样式"))?
        };
        self.cache.push(CachedFont {
            request: request.clone(),
            size,
            font,
        });
        Ok(&self.cache.last().unwrap().font)
    }
}

impl FontServices for MacFonts {
    fn add_resource(&mut self, name: &[u8]) -> Result<bool> {
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(name);
        if invalid || name.contains('\0') {
            return Err(Error::new(0, "字体资源名称不是有效 CP932 路径"));
        }
        let path = self.root.join(name.replace('\\', "/"));
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(Error::new(0, "字体资源读取失败")),
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(read::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::new(0, "字体资源读取失败"))?;
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "字体数据超过存储上限"));
        }
        Ok(self.add_data(&bytes).is_ok())
    }

    fn enumerate(&mut self, character_set: u32, variable_pitch: u32) -> Result<Vec<Vec<u16>>> {
        if character_set != u32::MAX {
            return Err(Error::new(0, "CoreText 尚未提供 GDI 字符集筛选"));
        }
        let mut names = Vec::new();
        for face in &self.faces {
            enumerate_font(&face.font, variable_pitch, &mut names)?;
        }
        let available = unsafe { Owned::new(ffi::CTFontManagerCopyAvailablePostScriptNames())? };
        let count = unsafe { ffi::CFArrayGetCount(available.get()) };
        for index in 0..count {
            let name = unsafe { ffi::CFArrayGetValueAtIndex(available.get(), index) };
            let font = unsafe { Owned::new(ffi::CTFontCreateWithName(name, 16.0, ptr::null()))? };
            enumerate_font(&font, variable_pitch, &mut names)?;
        }
        Ok(names)
    }

    fn validate(&mut self, request: &FontRequest) -> Result<()> {
        self.font(request, 16)?;
        Ok(())
    }

    fn measure(&mut self, request: &FontRequest, character: char, size: u32) -> Result<i32> {
        let font = self.font(request, size)?;
        if size == 0 {
            return Ok(0);
        }
        let measured = measured_advance(font, glyph(font, character)?)?;
        request.character_advance(character, size, measured)
    }

    fn rasterize(
        &mut self,
        request: &FontRequest,
        character: char,
        size: u32,
    ) -> Result<GlyphMask> {
        let font = self.font(request, size)?;
        if size == 0 {
            return GlyphMask::new([0; 2], [0; 2], 0, Vec::new());
        }
        let glyph = glyph(font, character)?;
        let measured = measured_advance(font, glyph)?;
        let advance = request.character_advance(character, size, measured)?;
        // 原绘字入口直接跳过半角及全角空格。
        if matches!(character, ' ' | '\u{3000}') {
            return GlyphMask::new([0; 2], [0; 2], advance, Vec::new());
        }
        let bounds = unsafe {
            ffi::CTFontGetBoundingRectsForGlyphs(font.get(), 1, &glyph, ptr::null_mut(), 1)
        };
        let left = integer(bounds.origin.x.floor())?;
        let bottom = integer(bounds.origin.y.floor())?;
        let right = integer((bounds.origin.x + bounds.size.width).ceil())?;
        let top = integer((bounds.origin.y + bounds.size.height).ceil())?;
        let width = right
            .checked_sub(left)
            .ok_or_else(|| Error::new(0, "字形尺寸超出支持范围"))?;
        let height = top
            .checked_sub(bottom)
            .ok_or_else(|| Error::new(0, "字形尺寸超出支持范围"))?;
        if width < 0
            || height < 0
            || (width as usize)
                .checked_mul(height as usize)
                .is_none_or(|n| n > read::MAX_BYTES / 4)
        {
            return Err(Error::new(0, "字形像素超过存储上限"));
        }
        if width == 0 || height == 0 {
            return GlyphMask::new([0; 2], [0; 2], advance, Vec::new());
        }
        let mut coverage = vec![0; width as usize * height as usize];
        unsafe {
            ffi::draw_glyph(
                font.get(),
                glyph,
                Point {
                    x: -f64::from(left),
                    y: -f64::from(bottom),
                },
                [width as usize, height as usize],
                &mut coverage,
            )?;
        }
        let baseline =
            integer(unsafe { ffi::CTFontGetAscent(font.get()) }.round())?.min(size as i32);
        let x = if request.measure_parameter != 0 && advance > measured {
            (advance - width) / 2 + 1
        } else {
            left.saturating_add(1)
        }
        .max(0);
        let y = baseline
            .checked_sub(top)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| Error::new(0, "字形位置超出支持范围"))?;
        for value in &mut coverage {
            *value = ((u32::from(*value) * 64 + 127) / 255) as u8;
        }
        GlyphMask::new([width as u32, height as u32], [x, y], advance, coverage)
    }
}

fn check_request(request: &FontRequest, size: u32) -> Result<()> {
    if request.backend != 0 {
        return Err(Error::new(0, "CoreText 不支持该字体后端"));
    }
    if !matches!(request.glyph_parameter as u8, 0 | 1 | 128) {
        return Err(Error::new(0, "CoreText 不支持该 GDI 字符集"));
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

fn same_name(left: &[u16], right: &[u16]) -> bool {
    String::from_utf16_lossy(left).to_lowercase() == String::from_utf16_lossy(right).to_lowercase()
}

fn cf_string(text: &[u16]) -> Result<Owned> {
    unsafe {
        Owned::new(ffi::CFStringCreateWithCharacters(
            ptr::null(),
            text.as_ptr(),
            text.len() as isize,
        ))
    }
}

fn string_units(text: &Owned) -> Result<Vec<u16>> {
    let count = unsafe { ffi::CFStringGetLength(text.get()) };
    if count < 0 || count as usize > read::MAX_BYTES / 2 {
        return Err(Error::new(0, "字体名称超过存储上限"));
    }
    let mut units = vec![0; count as usize];
    unsafe {
        ffi::CFStringGetCharacters(
            text.get(),
            ffi::Range {
                location: 0,
                length: count,
            },
            units.as_mut_ptr(),
        );
    }
    Ok(units)
}

fn font_names(font: &Owned) -> Result<Vec<Vec<u16>>> {
    let mut names = Vec::new();
    for key in unsafe {
        [
            ffi::kCTFontFamilyNameKey,
            ffi::kCTFontFullNameKey,
            ffi::kCTFontPostScriptNameKey,
        ]
    } {
        for localized in [false, true] {
            let raw = unsafe {
                if localized {
                    ffi::CTFontCopyLocalizedName(font.get(), key, ptr::null_mut())
                } else {
                    ffi::CTFontCopyName(font.get(), key)
                }
            };
            if !raw.is_null() {
                let value = string_units(&unsafe { Owned::new(raw)? })?;
                if !value.is_empty() && !names.contains(&value) {
                    names.push(value);
                }
            }
        }
    }
    Ok(names)
}

fn enumerate_font(font: &Owned, variable_pitch: u32, names: &mut Vec<Vec<u16>>) -> Result<()> {
    if variable_pitch == 0
        && unsafe { ffi::CTFontGetSymbolicTraits(font.get()) } & ffi::MONOSPACE == 0
    {
        return Ok(());
    }
    let name = unsafe { Owned::new(ffi::CTFontCopyName(font.get(), ffi::kCTFontFamilyNameKey))? };
    let mut name = string_units(&name)?;
    if name.first() == Some(&(b'@' as u16)) {
        return Ok(());
    }
    if name.len() > 63 {
        name.truncate(63);
    }
    if String::from_utf16(&name).is_err() {
        name.pop();
    }
    names.push(name);
    Ok(())
}

fn style_traits(font: &Owned) -> u32 {
    unsafe { ffi::CTFontGetSymbolicTraits(font.get()) & (ffi::BOLD | ffi::ITALIC) }
}

fn glyph(font: &Owned, character: char) -> Result<u16> {
    let mut input = [0; 2];
    let text = character.encode_utf16(&mut input);
    let mut glyphs = [0; 2];
    let found = unsafe {
        ffi::CTFontGetGlyphsForCharacters(
            font.get(),
            text.as_ptr(),
            glyphs.as_mut_ptr(),
            text.len() as isize,
        )
    };
    if !found || glyphs[0] == 0 {
        return Err(Error::new(
            0,
            format!("字体缺少字符 U+{:04X}", character as u32),
        ));
    }
    Ok(glyphs[0])
}

fn integer(value: f64) -> Result<i32> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(Error::new(0, "字形度量超出支持范围"));
    }
    Ok(value as i32)
}

fn measured_advance(font: &Owned, glyph: u16) -> Result<i32> {
    let mut size = Size {
        width: 0.0,
        height: 0.0,
    };
    unsafe {
        ffi::CTFontGetAdvancesForGlyphs(font.get(), 1, &glyph, &mut size, 1);
    }
    integer(size.width.round())
}
