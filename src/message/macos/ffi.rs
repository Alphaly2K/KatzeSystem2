use crate::{Error, Result};
use std::{ffi::c_void, ptr::NonNull};

type Ref = *const c_void;
pub(super) const ITALIC: u32 = 1;
pub(super) const BOLD: u32 = 1 << 1;
pub(super) const MONOSPACE: u32 = 1 << 10;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Point {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Size {
    pub width: f64,
    pub height: f64,
}
#[repr(C)]
pub(super) struct Rect {
    pub origin: Point,
    pub size: Size,
}
#[repr(C)]
pub(super) struct Range {
    pub location: isize,
    pub length: isize,
}

pub(super) struct Owned(NonNull<c_void>);
impl Owned {
    /// 接管 Create／Copy 返回的一份 CoreFoundation 所有权。
    pub unsafe fn new(value: Ref) -> Result<Self> {
        NonNull::new(value.cast_mut())
            .map(Self)
            .ok_or_else(|| Error::new(0, "平台字体对象创建失败"))
    }
    pub fn get(&self) -> Ref {
        self.0.as_ptr()
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // 所有 Owned 均来自对应的 Create／Copy 函数，且不复制所有权。
        unsafe {
            CFRelease(self.get());
        }
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(super) fn CFRelease(value: Ref);
    pub(super) fn CFDataCreate(allocator: Ref, bytes: *const u8, count: isize) -> Ref;
    pub(super) fn CFArrayGetCount(array: Ref) -> isize;
    pub(super) fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    pub(super) fn CFStringCreateWithCharacters(
        allocator: Ref,
        chars: *const u16,
        count: isize,
    ) -> Ref;
    pub(super) fn CFStringGetLength(string: Ref) -> isize;
    pub(super) fn CFStringGetCharacters(string: Ref, range: Range, buffer: *mut u16);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub(super) static kCTFontFamilyNameKey: Ref;
    pub(super) static kCTFontFullNameKey: Ref;
    pub(super) static kCTFontPostScriptNameKey: Ref;
    pub(super) fn CTFontManagerCreateFontDescriptorsFromData(data: Ref) -> Ref;
    pub(super) fn CTFontManagerCopyAvailablePostScriptNames() -> Ref;
    pub(super) fn CTFontCreateWithFontDescriptor(descriptor: Ref, size: f64, matrix: Ref) -> Ref;
    pub(super) fn CTFontCreateWithName(name: Ref, size: f64, matrix: Ref) -> Ref;
    pub(super) fn CTFontCreateUIFontForLanguage(ui_type: u32, size: f64, language: Ref) -> Ref;
    pub(super) fn CTFontCreateForString(font: Ref, string: Ref, range: Range) -> Ref;
    pub(super) fn CTFontCreateCopyWithAttributes(
        font: Ref,
        size: f64,
        matrix: Ref,
        descriptor: Ref,
    ) -> Ref;
    pub(super) fn CTFontCreateCopyWithSymbolicTraits(
        font: Ref,
        size: f64,
        matrix: Ref,
        traits: u32,
        mask: u32,
    ) -> Ref;
    pub(super) fn CTFontCopyName(font: Ref, name: Ref) -> Ref;
    pub(super) fn CTFontCopyLocalizedName(font: Ref, name: Ref, language: *mut Ref) -> Ref;
    pub(super) fn CTFontGetSymbolicTraits(font: Ref) -> u32;
    pub(super) fn CTFontGetGlyphsForCharacters(
        font: Ref,
        characters: *const u16,
        glyphs: *mut u16,
        count: isize,
    ) -> bool;
    pub(super) fn CTFontGetAscent(font: Ref) -> f64;
    pub(super) fn CTFontGetAdvancesForGlyphs(
        font: Ref,
        orientation: u32,
        glyphs: *const u16,
        advances: *mut Size,
        count: isize,
    ) -> f64;
    pub(super) fn CTFontGetBoundingRectsForGlyphs(
        font: Ref,
        orientation: u32,
        glyphs: *const u16,
        bounds: *mut Rect,
        count: isize,
    ) -> Rect;
    fn CTFontDrawGlyphs(
        font: Ref,
        glyphs: *const u16,
        positions: *const Point,
        count: usize,
        context: Ref,
    );
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceGray() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextRelease(context: Ref);
    fn CGContextSetGrayFillColor(context: Ref, gray: f64, alpha: f64);
    fn CGContextSetShouldAntialias(context: Ref, enabled: bool);
    fn CGContextSetShouldSmoothFonts(context: Ref, enabled: bool);
    fn CGContextSetShouldSubpixelPositionFonts(context: Ref, enabled: bool);
    fn CGContextSetShouldSubpixelQuantizeFonts(context: Ref, enabled: bool);
}

/// font 必须是活跃的 CTFont；pixels 长度必须等于 width × height，且两维非零。
pub(super) unsafe fn draw_glyph(
    font: Ref,
    glyph: u16,
    position: Point,
    size: [usize; 2],
    pixels: &mut [u8],
) -> Result<()> {
    // 位图只在本调用存活；释放上下文后才把覆盖交回调用者。
    unsafe {
        let space = CGColorSpaceCreateDeviceGray();
        if space.is_null() {
            return Err(Error::new(0, "灰度颜色空间创建失败"));
        }
        let context = CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            size[0],
            size[1],
            8,
            size[0],
            space,
            0,
        );
        CGColorSpaceRelease(space);
        if context.is_null() {
            return Err(Error::new(0, "字形位图创建失败"));
        }
        CGContextSetGrayFillColor(context, 1.0, 1.0);
        CGContextSetShouldAntialias(context, true);
        CGContextSetShouldSmoothFonts(context, false);
        CGContextSetShouldSubpixelPositionFonts(context, false);
        CGContextSetShouldSubpixelQuantizeFonts(context, false);
        CTFontDrawGlyphs(font, &glyph, &position, 1, context);
        CGContextRelease(context);
    }
    Ok(())
}
