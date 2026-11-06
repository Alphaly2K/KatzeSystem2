use super::{
    Alignment, Atom, AtomKind, Markup, Style, StyleDefaults, TextEncoding, TextGroup, Wait,
};
use crate::{Error, Result, read};

struct Parser<'a> {
    input: &'a str,
    characters: Vec<(usize, char)>,
    position: usize,
    defaults: &'a StyleDefaults,
    encoding: TextEncoding,
    style: Style,
    restore_color: u32,
    group: Option<TextGroup>,
    next_group: u32,
    atoms: Vec<Atom>,
    identifiers: std::collections::BTreeMap<i32, u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ParseState {
    pub style: Style,
    pub restore_color: u32,
    pub next_group: u32,
}

impl ParseState {
    pub fn new(defaults: &StyleDefaults) -> Self {
        Self {
            style: defaults.style(),
            restore_color: defaults.text_color,
            next_group: 0,
        }
    }
}

impl Markup {
    /// 按 CP932 标记规则解析正文，保留逐字符样式、不可断行组和原始等待操作。
    /// 已识别但尚未实现的标记返回其反斜杠位置；未知转义显示其后字符。
    pub fn parse(input: &str, defaults: &StyleDefaults) -> Result<Self> {
        Self::parse_with_encoding(input, defaults, TextEncoding::Cp932)
    }

    /// 输入为 UTF-8；encoding 选择标记中保留的原编码行为。
    pub fn parse_with_encoding(
        input: &str,
        defaults: &StyleDefaults,
        encoding: TextEncoding,
    ) -> Result<Self> {
        Self::parse_from(input, defaults, &mut ParseState::new(defaults), encoding)
    }

    pub(super) fn parse_from(
        input: &str,
        defaults: &StyleDefaults,
        state: &mut ParseState,
        encoding: TextEncoding,
    ) -> Result<Self> {
        if input.len() > read::MAX_BYTES {
            return Err(Error::new(0, "消息文本超过存储上限"));
        }
        let mut characters = Vec::new();
        for (offset, c) in input.char_indices() {
            if c == '\0' {
                return Err(Error::new(offset, "消息文本包含零字符"));
            }
            if characters.len() == read::MAX_ITEMS {
                return Err(Error::new(offset, "消息字符数量超过上限"));
            }
            characters.push((offset, c));
        }
        let mut parser = Parser {
            input,
            characters,
            position: 0,
            defaults,
            encoding,
            style: state.style,
            restore_color: state.restore_color,
            group: None,
            next_group: state.next_group,
            atoms: Vec::new(),
            identifiers: std::collections::BTreeMap::new(),
        };
        parser.run()?;
        *state = ParseState {
            style: parser.style,
            restore_color: parser.restore_color,
            next_group: parser
                .next_group
                .wrapping_add(u32::from(parser.group.is_some())),
        };
        Ok(Self {
            atoms: parser.atoms,
            identifiers: parser.identifiers,
            final_style: parser.style,
            restore_text_color: parser.restore_color,
        })
    }
}

impl Parser<'_> {
    fn offset(&self) -> usize {
        self.characters
            .get(self.position)
            .map_or(self.input.len(), |&(offset, _)| offset)
    }

    fn take(&mut self) -> char {
        match self.characters.get(self.position) {
            Some(&(_, c)) => {
                self.position += 1;
                c
            }
            None => '\0',
        }
    }

    fn rewind(&mut self) {
        self.position -= 1;
    }

    fn emit(&mut self, start: usize, kind: AtomKind, group: Option<TextGroup>) {
        self.atoms.push(Atom {
            source: start..self.offset(),
            kind,
            style: self.style,
            group,
        });
    }

    fn character(&mut self, start: usize, c: char) {
        self.emit(
            start,
            if c == '\0' {
                AtomKind::NewLine
            } else {
                AtomKind::Character(c)
            },
            self.group,
        );
    }

    fn run(&mut self) -> Result<()> {
        while self.position < self.characters.len() {
            let start = self.offset();
            let c = self.take();
            match (c, self.group) {
                ('\\', _) => self.escape(start)?,
                ('[', None) => {
                    self.group = Some(TextGroup {
                        index: self.next_group,
                        ruby: false,
                    });
                }
                ('/', Some(group)) if !group.ruby => {
                    self.group.as_mut().unwrap().ruby = true;
                }
                (']', Some(_)) => {
                    self.group = None;
                    self.next_group += 1;
                }
                _ => self.character(start, c),
            }
        }
        Ok(())
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.position;
        let c = self.characters.get(start)?.1;
        if !c.is_ascii_digit() {
            return None;
        }
        let radix = if c == '0' && matches!(self.characters.get(start + 1), Some((_, 'x' | 'X'))) {
            self.position += 2;
            16
        } else {
            10
        };
        let mut value = 0u32;
        while let Some(&(_, c)) = self.characters.get(self.position) {
            let Some(digit) = c.to_digit(radix).filter(|_| c.is_ascii()) else {
                break;
            };
            value = value.wrapping_mul(radix).wrapping_add(digit);
            self.position += 1;
        }
        if matches!(self.characters.get(self.position), Some((_, ';'))) {
            self.position += 1;
        }
        Some(value)
    }

    fn escape(&mut self, start: usize) -> Result<()> {
        let c = self.take();
        match c.to_ascii_lowercase() {
            'c' => self.color(),
            'f' => self.font(),
            'n' => self.emit(start, AtomKind::NewLine, None),
            'r' => self.emit(start, AtomKind::Character('\u{8}'), None),
            's' => {
                self.emit(
                    start,
                    AtomKind::Character(match self.encoding {
                        TextEncoding::Cp932 => '\u{3000}',
                        // Unicode 入口保留旧字符常量，直接登记为 U+8140。
                        TextEncoding::Unicode => '\u{8140}',
                    }),
                    None,
                );
                self.emit(start, AtomKind::NewLine, None);
            }
            '@' => self.emit(start, AtomKind::Continue, None),
            'p' => {
                let suffix = self.take().to_ascii_lowercase();
                let alignment = match suffix {
                    'c' => Alignment::Center,
                    'r' => Alignment::Right,
                    'l' => Alignment::Left,
                    _ => {
                        self.rewind();
                        Alignment::Left
                    }
                };
                self.emit(start, AtomKind::Align(alignment), None);
            }
            'x' | 'y' => {
                if let Some(value) = self.number() {
                    let group = if c.eq_ignore_ascii_case(&'x') && matches!(value, 1 | 2) {
                        Some(TextGroup {
                            index: self.next_group,
                            ruby: value == 2,
                        })
                    } else {
                        None
                    };
                    self.emit(
                        start,
                        if c.eq_ignore_ascii_case(&'x') {
                            AtomKind::X(value)
                        } else {
                            AtomKind::Y(value)
                        },
                        group,
                    );
                }
            }
            'w' => {
                let suffix = self.take().to_ascii_lowercase();
                let wait = match suffix {
                    'r' => Some(Wait::Reset),
                    'f' => self.number().map(Wait::Frames),
                    _ => {
                        self.rewind();
                        self.number().map(Wait::Characters)
                    }
                };
                if let Some(wait) = wait {
                    self.emit(start, AtomKind::Wait(wait), None);
                }
            }
            'i' if self
                .characters
                .get(self.position)
                .is_some_and(|&(_, c)| c.eq_ignore_ascii_case(&'d')) =>
            {
                self.position += 1;
                let mut value = 0;
                let key = if let Some(key) = self.number() {
                    if self.take() == ':' {
                        value = self.number().unwrap_or(0);
                    } else {
                        // 数值恰好结束输入时也回退，最后一个字符会再次解析。
                        self.rewind();
                    }
                    key
                } else {
                    0
                };
                self.identifiers.entry(key as i32).or_insert(value);
            }
            'i' => self.emit(start, AtomKind::Character('\u{6}'), None),
            'u' if self.encoding == TextEncoding::Unicode => {
                if self
                    .characters
                    .get(self.position)
                    .is_some_and(|&(_, c)| c.eq_ignore_ascii_case(&'i'))
                {
                    return Err(Error::new(start, "Unicode 用户字形标记尚未接入"));
                }
                self.emit(start, AtomKind::Character('\u{6}'), None);
            }
            '$' | 't' | 'z' => {
                return Err(Error::new(start, "消息标记尚未实现"));
            }
            _ => self.character(start, c),
        }
        Ok(())
    }

    fn color(&mut self) {
        let suffix = self.take().to_ascii_lowercase();
        match suffix {
            'r' => {
                self.style.ruby_color = self.number().unwrap_or_else(|| {
                    let c = self.take();
                    if c == ';' && self.position < self.characters.len() {
                        self.position += 1;
                    }
                    self.defaults.ruby_color
                });
            }
            's' => self.style.shadow_color = self.number().unwrap_or(self.defaults.shadow_color),
            'd' => self.restore_color = self.number().unwrap_or(self.defaults.text_color),
            '\0' => self.style.text_color = self.restore_color,
            _ => {
                self.rewind();
                self.style.text_color = self.number().unwrap_or_else(|| {
                    if suffix == ';' {
                        self.position += 1;
                    }
                    self.restore_color
                });
            }
        }
    }

    fn font(&mut self) {
        let suffix = self.take().to_ascii_lowercase();
        match suffix {
            'r' => {
                self.style.ruby_size = self.number().unwrap_or_else(|| {
                    let c = self.take();
                    if c == ';' && self.position < self.characters.len() {
                        self.position += 1;
                    }
                    self.defaults.ruby_sizes[2]
                });
            }
            's' | 'l' => {
                let next = self.take().to_ascii_lowercase();
                let index = match (suffix, next) {
                    ('s', 's') => 0,
                    ('s', _) => 1,
                    ('l', 'l') => 4,
                    _ => 3,
                };
                self.style.text_size = self.defaults.text_sizes[index];
                self.style.ruby_size = self.defaults.ruby_sizes[index];
                if next == suffix {
                    let after = self.take();
                    if after != ';' && after != '\0' {
                        self.rewind();
                    }
                } else if next != ';' && next != '\0' {
                    self.rewind();
                }
            }
            'n' => {
                self.style.text_size = self.defaults.text_sizes[2];
                self.style.ruby_size = self.defaults.ruby_sizes[2];
            }
            '+' | '-' => {
                if let Some(value) = self.number() {
                    self.style.text_size = if suffix == '+' {
                        self.style.text_size.wrapping_add(value)
                    } else {
                        self.style.text_size.wrapping_sub(value)
                    };
                } else {
                    self.style.text_size = self.defaults.text_sizes[2];
                }
            }
            _ => {
                self.rewind();
                self.style.text_size = self.number().unwrap_or_else(|| {
                    if suffix == ';' {
                        self.position += 1;
                    }
                    self.defaults.text_sizes[2]
                });
            }
        }
    }
}
