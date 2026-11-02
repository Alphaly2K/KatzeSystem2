//! 消息标记与供布局使用的文字片段。

use std::ops::Range;

mod display;
mod effects;
mod font;
mod layout;
mod line_breaks;
mod markup;
mod page;
mod playback;
mod reveal;
mod timing;

pub use display::{Display, DisplayProgress, DisplaySettings};
pub use effects::GlyphEffects;
pub use font::{Font, GlyphMask, GlyphPaint};
pub use layout::{Layout, LayoutEntry, LayoutSettings, VerticalAlignment};
pub use line_breaks::BreakCharacters;
pub use page::Page;
pub use playback::{DisplayUpdate, GlyphState};
pub use reveal::{Reveal, RevealGlyph};
pub use timing::{Schedule, TimingSettings};

/// 已解码文字仍保留原文字对象路径，供布局及消息更新使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Cp932,
    Unicode,
}

/// 颜色保留消息语法中的 AABBGGRR；绘制时调用 message_color_argb 换算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub text_size: u32,
    pub ruby_size: u32,
    pub text_color: u32,
    pub ruby_color: u32,
    pub shadow_color: u32,
}

/// 字号依次为最小、小、普通、大、最大，由消息对象配置提供。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleDefaults {
    pub text_sizes: [u32; 5],
    pub ruby_sizes: [u32; 5],
    pub text_color: u32,
    pub ruby_color: u32,
    pub shadow_color: u32,
}

impl StyleDefaults {
    pub fn style(&self) -> Style {
        Style {
            text_size: self.text_sizes[2],
            ruby_size: self.ruby_sizes[2],
            text_color: self.text_color,
            ruby_color: self.ruby_color,
            shadow_color: self.shadow_color,
        }
    }
}

pub fn message_color_argb(aabbggrr: u32) -> u32 {
    (aabbggrr & 0xff00_ff00) | ((aabbggrr & 0xff) << 16) | ((aabbggrr >> 16) & 0xff)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// 同一 index 标识同组正文和注音；布局按可用宽度维护其分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextGroup {
    pub index: u32,
    pub ruby: bool,
}

/// 原始等待参数；时长换算和结束条件由消息对象提供。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Characters(u32),
    Frames(u32),
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomKind {
    Character(char),
    NewLine,
    Align(Alignment),
    X(u32),
    Y(u32),
    Continue,
    Wait(Wait),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    /// 当前输入字符串中的 UTF-8 字节区间，包含该片段消费的转义。
    pub source: Range<usize>,
    pub kind: AtomKind,
    pub style: Style,
    pub group: Option<TextGroup>,
}

/// 一段完整输入的标记结果；不查询字体、设备或系统时间。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Markup {
    pub atoms: Vec<Atom>,
    pub final_style: Style,
    /// cd 标记修改的正文恢复颜色。
    pub restore_text_color: u32,
}
