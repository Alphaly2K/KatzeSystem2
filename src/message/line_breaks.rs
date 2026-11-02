use super::{AtomKind, Layout, LayoutEntry, LayoutSettings, TextEncoding};

/// 各集合保存解码后的字符；调用者可在基线默认值上追加配置字符。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakCharacters {
    /// 避免出现在新行开头的字符。
    pub no_start: String,
    /// 避免留在上一行结尾的字符。
    pub no_end: String,
    /// 连续相同字符按一组调整。
    pub repeated: String,
    /// 建立缩进宽度，并在后续行首暂时取消缩进的字符。
    pub indent_open: String,
    /// 在行首保留当前缩进开关的字符。
    pub indent_keep: String,
}

impl Default for BreakCharacters {
    fn default() -> Self {
        Self {
            no_start: concat!(
                "　 、。，．・？！゛゜ヽヾゝゞ々ー－＝～：；）］｝」』!),.:;?]}｡｣､･ｰﾞﾟ♪",
                "ァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎ",
                "①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳"
            )
            .into(),
            no_end: "（［｛「『([{｢".into(),
            repeated: "…‥―".into(),
            indent_open: "（［｛「『([{｢".into(),
            indent_keep: "（［｛「([{｢".into(),
        }
    }
}

impl BreakCharacters {
    /// 选择该文字对象路径的默认集合；Default 使用 CP932 集合。
    pub fn for_encoding(encoding: TextEncoding) -> Self {
        let mut characters = Self::default();
        if encoding == TextEncoding::Unicode {
            characters.no_start.push_str("”―");
            characters.no_end.push('“');
        }
        characters
    }
}

impl LayoutEntry {
    pub(super) fn in_set(&self, characters: &str) -> bool {
        matches!(self.kind, AtomKind::Character(c) if characters.contains(c))
    }
}

impl Layout {
    pub(super) fn first_glyph(&self, start: usize) -> Option<usize> {
        (start..self.entries.len()).find(|&i| self.entries[i].glyph())
    }

    pub(super) fn adjust_break(
        &self,
        start: usize,
        mut end: usize,
        remaining: i32,
        settings: &LayoutSettings,
    ) -> usize {
        while end > start && self.entries[end].in_set(&settings.break_characters.no_end) {
            end -= 1;
        }
        let extend = self.extend_punctuation(start, &mut end, remaining, settings);
        if extend || (settings.encoding == TextEncoding::Unicode && settings.word_wrap) {
            self.join_group(start, &mut end, remaining, settings);
        }
        end
    }

    fn extend_punctuation(
        &self,
        start: usize,
        end: &mut usize,
        mut remaining: i32,
        settings: &LayoutSettings,
    ) -> bool {
        if self.entries[*end].line_break(settings.encoding) {
            return false;
        }
        let characters = &settings.break_characters.no_start;
        let mut previous = None;
        for i in *end + 1..self.entries.len() {
            let entry = &self.entries[i];
            if entry.line_break(settings.encoding) {
                *end += 1;
                return true;
            }
            if entry.glyph() && !entry.in_set(characters) {
                return true;
            }
            let unicode = settings.encoding == TextEncoding::Unicode;
            let period = |kind| {
                kind == AtomKind::Character('。') || (unicode && kind == AtomKind::Character('.'))
            };
            let after_period = previous.is_some_and(period) && !period(entry.kind);
            if after_period && !(unicode && entry.kind == AtomKind::Character('」')) {
                for (i, entry) in self.entries.iter().enumerate().skip(i) {
                    if !entry.line_break(settings.encoding) && entry.code() != 0xffff {
                        break;
                    }
                    *end = i;
                }
                return false;
            }
            let space = !after_period
                && match entry.kind {
                    AtomKind::Character(c) if unicode => matches!(c, ' ' | '\u{8140}'),
                    AtomKind::Character('　') => true,
                    _ => false,
                };
            if entry.size[0] < remaining || space {
                *end += 1;
                remaining = remaining
                    .wrapping_sub(entry.size[0])
                    .wrapping_sub(settings.character_spacing);
                previous = Some(entry.kind);
                continue;
            }
            let original = *end;
            for _ in 0..4 {
                if *end <= start {
                    return true;
                }
                let entry = &self.entries[*end];
                if entry.glyph() && !entry.in_set(characters) {
                    *end -= 1;
                    return true;
                }
                *end -= 1;
            }
            if *end > start {
                *end = original;
            }
            return true;
        }
        true
    }

    fn join_group(
        &self,
        start: usize,
        end: &mut usize,
        mut remaining: i32,
        settings: &LayoutSettings,
    ) {
        let original = *end;
        if settings.encoding == TextEncoding::Unicode && settings.word_wrap {
            self.join_word(start, end, remaining, settings);
            return;
        }
        let mut before = original;
        let mut group = None;
        let mut character = None;
        for i in (start + 1..=original).rev() {
            let entry = &self.entries[i];
            if entry.glyph() {
                if !entry.in_set(&settings.break_characters.repeated) {
                    let Some(index) = entry.group.map(|group| group.index) else {
                        break;
                    };
                    if group.is_some_and(|group| group != index) {
                        break;
                    }
                    group = Some(index);
                }
                character = Some(entry.kind);
            }
            before = i - 1;
        }
        let Some(character) = character else {
            return;
        };
        let mut next = original + 1;
        let mut continued = false;
        while next < self.entries.len() {
            let entry = &self.entries[next];
            if entry.glyph() {
                let matches = match group {
                    Some(index) => entry.group.is_some_and(|group| group.index == index),
                    None => entry.kind == character,
                };
                if !matches {
                    break;
                }
                continued = true;
            }
            next += 1;
        }
        if !continued {
            return;
        }
        // 此处只累计正文宽度，不叠加字符间距。
        let width = self.entries[original + 1..next]
            .iter()
            .filter(|entry| !entry.ruby())
            .fold(0i32, |sum, entry| sum.wrapping_add(entry.size[0]));
        if width >= remaining {
            *end = before;
            return;
        }
        *end = next - 1;
        remaining = remaining.wrapping_sub(width);
        for i in next..self.entries.len() {
            let entry = &self.entries[i];
            if entry.glyph() {
                if !entry.in_set(&settings.break_characters.no_start) {
                    break;
                }
                if entry.size[0] > remaining {
                    *end = before;
                    break;
                }
                *end = i;
                remaining = remaining.wrapping_sub(entry.size[0]);
            } else if entry.line_break(settings.encoding) {
                *end = i;
            }
        }
    }

    fn join_word(&self, start: usize, end: &mut usize, remaining: i32, settings: &LayoutSettings) {
        let original = *end;
        if self.entries[original].kind == AtomKind::Character(' ')
            || self.entries[original].line_break(settings.encoding)
            || original + 1 == self.entries.len()
        {
            return;
        }
        let next = (original + 1..self.entries.len())
            .find(|&i| self.entries[i].kind == AtomKind::Character(' '))
            .map_or(self.entries.len(), |i| i + 1);
        let width = self.entries[original + 1..next]
            .iter()
            .filter(|entry| !entry.ruby())
            .fold(0i32, |sum, entry| sum.wrapping_add(entry.size[0]));
        if width >= remaining {
            if start < original {
                *end = (start + 1..original)
                    .rev()
                    .find(|&i| self.entries[i].kind == AtomKind::Character(' '))
                    .unwrap_or(start);
            }
            return;
        }
        *end = next - 1;
        let mut remaining = remaining.wrapping_sub(width);
        for i in next..self.entries.len() {
            let entry = &self.entries[i];
            if entry.glyph() {
                if !entry.in_set(&settings.break_characters.no_start) {
                    break;
                }
                if entry.size[0] > remaining {
                    if let Some(space) = (1..=i)
                        .rev()
                        .find(|&i| self.entries[i].kind == AtomKind::Character(' '))
                    {
                        *end = space;
                    }
                    break;
                }
                *end = i;
                remaining = remaining.wrapping_sub(entry.size[0]);
            } else if entry.line_break(settings.encoding) {
                *end = i;
            }
        }
    }
}
