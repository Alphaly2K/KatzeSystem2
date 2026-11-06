use super::{Alignment, AtomKind, BreakCharacters, Markup, TextEncoding, TextGroup, Wait};
use crate::{Error, Result, read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    Preserve,
    Top,
    Bottom,
}

/// 逻辑窗口宽度、左／上／右边距和消息局部偏移分别保存。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutSettings {
    pub encoding: TextEncoding,
    pub width: i32,
    pub margins: [i32; 3],
    pub offset: [i32; 2],
    pub character_spacing: i32,
    pub line_spacing: i32,
    pub normal_size: i32,
    pub ruby_offset: i32,
    pub vertical_alignment: VerticalAlignment,
    pub line_breaking: bool,
    /// Unicode 路径按半角空格保持单词；CP932 路径忽略此开关。
    pub word_wrap: bool,
    pub hanging_indent: bool,
    pub break_characters: BreakCharacters,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutEntry {
    /// 对应 Markup::atoms 的下标；自动插入的换行使用 None。
    pub atom: Option<usize>,
    pub kind: AtomKind,
    pub position: [i32; 2],
    pub size: [i32; 2],
    pub line: u32,
    pub group: Option<TextGroup>,
    /// 注音绑定的正文布局条目，供逐字显示使用。
    pub display_anchor: Option<usize>,
}

impl LayoutEntry {
    pub(super) fn code(&self) -> u32 {
        match self.kind {
            AtomKind::Character(c) => c as u32,
            AtomKind::NewLine => 0,
            AtomKind::Align(Alignment::Left) => 1,
            AtomKind::Align(Alignment::Center) => 2,
            AtomKind::Align(Alignment::Right) => 3,
            AtomKind::X(_) => 4,
            AtomKind::Y(_) => 5,
            AtomKind::Continue => 7,
            AtomKind::Wait(_) => 9,
        }
    }

    pub(super) fn glyph(&self) -> bool {
        self.code() > 9 && self.code() != 0xffff
    }

    pub(super) fn ruby(&self) -> bool {
        self.group.is_some_and(|group| group.ruby)
    }

    pub(super) fn line_break(&self, encoding: TextEncoding) -> bool {
        self.code() == 0 || (encoding == TextEncoding::Unicode && self.code() == 8)
    }
}

/// 已度量的文字及控制记录，包含自动换行与注音显示关联。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub entries: Vec<LayoutEntry>,
    /// 水平布局结束时最后一个文字记录的右边缘和顶部。
    pub cursor: Option<[i32; 2]>,
    /// 水平布局记录的最大下边缘；后续垂直对齐和注音不重算它。
    pub bottom: i32,
}

impl Markup {
    /// 度量闭包接受字符及该字符的正文／注音字号，返回原始整数前进宽度。
    pub fn layout(
        &self,
        settings: &LayoutSettings,
        measure: impl FnMut(char, u32) -> Result<i32>,
    ) -> Result<Layout> {
        let mut layout = Layout {
            entries: self.entries(measure)?,
            cursor: None,
            bottom: 0,
        };
        layout.arrange(settings)?;
        Ok(layout)
    }

    pub(super) fn entries(
        &self,
        mut measure: impl FnMut(char, u32) -> Result<i32>,
    ) -> Result<Vec<LayoutEntry>> {
        if self.atoms.len() > read::MAX_ITEMS {
            return Err(Error::new(0, "消息布局输入条目数量超过上限"));
        }
        let mut entries = Vec::new();
        for (index, atom) in self.atoms.iter().enumerate() {
            if matches!(
                atom.kind,
                AtomKind::Wait(Wait::Characters(_) | Wait::Frames(_))
            ) {
                continue;
            }
            let mut group = atom.group;
            let mut size = [
                0,
                if group.is_some_and(|group| group.ruby) {
                    atom.style.ruby_size
                } else {
                    atom.style.text_size
                } as i32,
            ];
            let mut position = [0; 2];
            match atom.kind {
                AtomKind::Character(c) if c as u32 > 9 && c as u32 != 0xffff => {
                    size[0] = measure(c, size[1] as u32)?;
                }
                AtomKind::X(value) => {
                    // 此标记的数值进入原记录构造的类别参数，坐标参数保持零。
                    group = match value {
                        0 => None,
                        1 | 2 => Some(TextGroup {
                            index: atom.group.map_or(0, |group| group.index),
                            ruby: value == 2,
                        }),
                        _ => {
                            size = [0; 2];
                            Some(TextGroup {
                                index: 0,
                                ruby: false,
                            })
                        }
                    };
                    if value == 2 {
                        size[1] = atom.style.ruby_size as i32;
                    }
                }
                AtomKind::Y(value) => position[0] = value as i32,
                _ => (),
            }
            entries.push(LayoutEntry {
                atom: Some(index),
                kind: atom.kind,
                position,
                size,
                line: 0,
                group,
                display_anchor: None,
            });
        }
        Ok(entries)
    }
}

impl Layout {
    pub(super) fn arrange(&mut self, settings: &LayoutSettings) -> Result<()> {
        self.horizontal(settings)?;
        self.vertical(settings);
        self.ruby(settings)
    }

    fn horizontal(&mut self, settings: &LayoutSettings) -> Result<()> {
        let [left, top, right] = settings.margins;
        let right = settings.width.wrapping_sub(right);
        let mut y = top.wrapping_add(settings.offset[1]);
        let mut start = 0;
        let mut line = 0;
        let mut last_size = 0;
        let mut alignment = Alignment::Left;
        let rules = &settings.break_characters;
        let indent = if settings.hanging_indent {
            self.first_glyph(0)
                .filter(|&i| self.entries[i].in_set(&rules.indent_open))
                .map_or(0, |i| {
                    self.entries[i].size[0].wrapping_add(settings.character_spacing)
                })
        } else {
            0
        };
        let mut indented = false;
        let mut seen_glyph = false;
        while start < self.entries.len() {
            if seen_glyph {
                let first = self.first_glyph(start);
                if settings.hanging_indent
                    && first.is_some_and(|i| self.entries[i].in_set(&rules.indent_open))
                {
                    indented = false;
                } else if !settings.hanging_indent
                    || first.is_none_or(|i| !self.entries[i].in_set(&rules.indent_keep))
                {
                    indented = true;
                }
            }
            let mut shift = if indented { indent } else { 0 };
            if last_size == 0 {
                last_size = if self.entries[start].glyph() {
                    self.entries[start].size[1]
                } else {
                    settings.normal_size
                };
            }
            let mut height = last_size;
            let mut end = start;
            let mut next = start;
            let mut x = left.wrapping_add(settings.offset[0]).wrapping_add(shift);
            let mut first_character = y != top;
            let mut wrapped = false;
            let mut reset_alignment = false;
            while next < self.entries.len() {
                let entry = &mut self.entries[next];
                seen_glyph |= entry.glyph();
                match entry.code() {
                    _ if entry.line_break(settings.encoding) => {
                        end = next;
                        next += 1;
                        break;
                    }
                    6 => {
                        // 先结束扫描，随后由禁则调整决定是否消费当前记录。
                        indented = false;
                        shift = 0;
                        reset_alignment = true;
                        break;
                    }
                    4 => x = entry.position[0].wrapping_add(settings.offset[0]),
                    5 => y = entry.position[1].wrapping_add(settings.offset[1]),
                    _ if !entry.ruby() => {
                        if x.wrapping_add(entry.size[0]) > right {
                            wrapped = true;
                            break;
                        }
                        if settings.encoding == TextEncoding::Cp932
                            && first_character
                            && entry.kind == AtomKind::Character('\u{3000}')
                        {
                            entry.size[0] = 0;
                        } else {
                            x = x
                                .wrapping_add(entry.size[0])
                                .wrapping_add(settings.character_spacing);
                        }
                        first_character = false;
                    }
                    _ => (),
                }
                end = next;
                next += 1;
            }
            if next > start
                && self.entries[next - 1].line_break(settings.encoding)
                && self
                    .entries
                    .get(next)
                    .is_some_and(|entry| entry.code() == 6)
            {
                end = next;
                next += 1;
                indented = false;
                shift = 0;
                reset_alignment = true;
            }
            if settings.line_breaking {
                end = self.adjust_break(start, end, settings.width.wrapping_sub(x), settings);
                next = end + 1;
                wrapped &= !self.entries[end].line_break(settings.encoding);
            }
            wrapped &= next < self.entries.len();
            let mut x = left.wrapping_add(settings.offset[0]).wrapping_add(shift);
            let mut remaining = right.wrapping_sub(left).wrapping_sub(shift);
            let mut i = start;
            while i <= end {
                if alignment != Alignment::Left {
                    let mut width = 0i32;
                    for entry in &self.entries[i..=end] {
                        if !entry.ruby() {
                            width = width
                                .wrapping_add(entry.size[0])
                                .wrapping_add(settings.character_spacing);
                            if matches!(entry.kind, AtomKind::Align(a) if a != alignment) {
                                break;
                            }
                        }
                    }
                    let free = remaining.wrapping_sub(width).max(0);
                    let shift = if alignment == Alignment::Right {
                        free
                    } else {
                        free / 2
                    };
                    x = x.wrapping_add(shift);
                    remaining = if alignment == Alignment::Right {
                        0
                    } else {
                        free - shift
                    };
                }
                while i <= end {
                    let entry = &mut self.entries[i];
                    let code = entry.code();
                    if alignment == Alignment::Left && matches!(code, 4 | 5) {
                        if code == 4 {
                            x = entry.position[0];
                            remaining = right.wrapping_sub(x);
                        } else {
                            y = entry.position[1];
                        }
                    } else {
                        entry.position = [x, y];
                        if entry.glyph() {
                            self.cursor = Some([x.wrapping_add(entry.size[0]), y]);
                            self.bottom = self.bottom.max(y.wrapping_add(entry.size[1]));
                        }
                        if !entry.ruby() {
                            let advance = entry.size[0].wrapping_add(settings.character_spacing);
                            x = x.wrapping_add(advance);
                            if entry.glyph() {
                                last_size = entry.size[1];
                            } else if let AtomKind::Align(next_alignment) = entry.kind {
                                if next_alignment != alignment {
                                    alignment = next_alignment;
                                    i += 1;
                                    break;
                                }
                            }
                            height = height.max(last_size);
                            if alignment == Alignment::Left {
                                remaining = remaining.wrapping_sub(advance).max(0);
                            }
                        }
                    }
                    i += 1;
                }
            }
            for entry in &mut self.entries[start..=end] {
                entry.line = line;
            }
            if reset_alignment {
                alignment = Alignment::Left;
            }
            if wrapped {
                if self.entries.len() == read::MAX_ITEMS {
                    return Err(Error::new(0, "消息布局条目数量超过上限"));
                }
                self.entries.insert(
                    end + 1,
                    LayoutEntry {
                        atom: None,
                        kind: AtomKind::NewLine,
                        position: [0; 2],
                        size: [0; 2],
                        line: 0,
                        group: None,
                        display_anchor: None,
                    },
                );
                next += 1;
            }
            if next == start {
                break;
            }
            start = next;
            y = y.wrapping_add(height).wrapping_add(settings.line_spacing);
            line += 1;
        }
        Ok(())
    }

    fn vertical(&mut self, settings: &LayoutSettings) {
        let mut y = settings.margins[1].wrapping_add(settings.offset[1]);
        let mut start = 0;
        while start < self.entries.len() {
            let end = (start..self.entries.len())
                .find(|&i| self.entries[i].line_break(settings.encoding))
                .unwrap_or(self.entries.len());
            let mut height = self.entries[start..end]
                .iter()
                .filter(|entry| entry.glyph())
                .map(|entry| entry.size[1])
                .fold(0, i32::max);
            if height == 0 && end < self.entries.len() {
                height = self.entries[end].size[1];
            }
            for entry in &mut self.entries[start..end] {
                match settings.vertical_alignment {
                    VerticalAlignment::Preserve => (),
                    VerticalAlignment::Top => entry.position[1] = y,
                    VerticalAlignment::Bottom => {
                        entry.position[1] = y.wrapping_add(height).wrapping_sub(entry.size[1])
                    }
                }
            }
            y = y.wrapping_add(height).wrapping_add(settings.line_spacing);
            start = end + 1;
        }
    }

    fn ruby(&mut self, settings: &LayoutSettings) -> Result<()> {
        let groups = self
            .entries
            .iter()
            .filter_map(|entry| entry.group.map(|group| group.index))
            .collect::<std::collections::BTreeSet<_>>();
        for group in groups {
            let Some(start) = self
                .entries
                .iter()
                .position(|entry| entry.group.is_some_and(|g| g.index == group))
            else {
                continue;
            };
            let middle = (start..self.entries.len())
                .find(|&i| {
                    self.entries[i]
                        .group
                        .is_none_or(|g| g.index != group || g.ruby)
                })
                .unwrap_or(self.entries.len());
            if middle == self.entries.len()
                || self.entries[middle].group.is_none_or(|g| g.index != group)
            {
                continue;
            }
            let end = (middle..self.entries.len())
                .find(|&i| self.entries[i].group.is_none_or(|g| g.index != group))
                .unwrap_or(self.entries.len());
            let base_width = self.entries[start..middle]
                .iter()
                .fold(0i32, |sum, e| sum.wrapping_add(e.size[0]));
            let mut base_x = -1;
            for entry in &self.entries[start..middle] {
                if base_x < 0 && entry.glyph() {
                    base_x = entry.position[0];
                }
            }
            let ruby_width = self.entries[middle..end]
                .iter()
                .fold(0i32, |sum, e| sum.wrapping_add(e.size[0]));
            let glyphs = self.entries[middle..end]
                .iter()
                .filter(|e| e.glyph())
                .count();
            let mut x = base_x;
            if (base_width as u32) < ruby_width as u32 {
                let centered = base_x
                    .wrapping_sub((ruby_width.wrapping_sub(base_width) as u32 >> 1) as i32)
                    .max(0);
                x = if (centered.wrapping_add(ruby_width) as u32) < settings.width as u32 {
                    centered
                } else {
                    settings.width.wrapping_sub(ruby_width)
                };
                for entry in &mut self.entries[middle..end] {
                    entry.position[0] = x;
                    if entry.glyph() {
                        x = x.wrapping_add(entry.size[0]);
                    }
                }
            } else {
                let mut slack = base_width.wrapping_sub(ruby_width) as u32;
                let mut gaps = glyphs as u32 + 1;
                for entry in &mut self.entries[middle..end] {
                    let gap = slack / gaps;
                    entry.position[0] = x.wrapping_add(gap as i32);
                    if entry.glyph() {
                        slack -= gap;
                        x = x.wrapping_add(entry.size[0]).wrapping_add(gap as i32);
                        gaps -= 1;
                    }
                }
            }
            if middle == 0 {
                return Err(Error::new(0, "消息注音缺少可绑定的前一条记录"));
            }
            for i in middle..end {
                let mut anchor = middle - 1;
                while anchor > start
                    && self.entries[i].position[0] <= self.entries[anchor - 1].position[0]
                {
                    anchor -= 1;
                }
                self.entries[i].position[1] = self.entries[anchor].position[1]
                    .wrapping_sub(self.entries[i].size[1])
                    .wrapping_add(settings.ruby_offset);
                self.entries[i].display_anchor = if self.entries[anchor].ruby() {
                    self.entries[anchor].display_anchor
                } else {
                    Some(anchor)
                };
            }
        }
        Ok(())
    }
}
