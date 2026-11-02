use super::markup::ParseState;
use super::{
    AtomKind, Layout, LayoutSettings, Markup, Schedule, StyleDefaults, TextEncoding, TimingSettings,
};
use crate::{Error, Result, read};

/// 持有一页消息的来源、解析状态、布局及显示时间；追加文字在 prepare 时解析。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    defaults: StyleDefaults,
    encoding: TextEncoding,
    source: String,
    characters: usize,
    parsed_bytes: usize,
    state: ParseState,
    markup: Markup,
    layout: Layout,
    atom_times: Vec<Option<u32>>,
    schedule: Schedule,
    dirty: bool,
}

impl Page {
    pub fn new(defaults: StyleDefaults, encoding: TextEncoding) -> Self {
        Self {
            defaults,
            encoding,
            source: String::new(),
            characters: 0,
            parsed_bytes: 0,
            state: ParseState::new(&defaults),
            markup: Markup {
                atoms: Vec::new(),
                final_style: defaults.style(),
                restore_text_color: defaults.text_color,
            },
            layout: Layout {
                entries: Vec::new(),
                cursor: None,
                bottom: 0,
            },
            atom_times: Vec::new(),
            schedule: Schedule {
                deadlines: Vec::new(),
                accumulated_units: 0,
            },
            dirty: true,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// 仅包含最近一次成功 prepare 的文字，来源区间指向整页 source。
    pub fn markup(&self) -> &Markup {
        &self.markup
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 同一次 prepare 前的追加可共同组成一个标记或分组。
    pub fn append(&mut self, text: &str) -> Result<()> {
        let offset = self.source.len();
        if text.len() > read::MAX_BYTES - offset {
            return Err(Error::new(offset, "消息页文本超过存储上限"));
        }
        if let Some(zero) = text.find('\0') {
            return Err(Error::new(offset + zero, "消息文本包含零字符"));
        }
        let count = text.chars().count();
        if count > read::MAX_ITEMS - self.characters {
            return Err(Error::new(offset, "消息页字符数量超过上限"));
        }
        self.source.push_str(text);
        self.characters += count;
        self.dirty = true;
        Ok(())
    }

    /// 改变布局配置后调用；既有记录的显示时间保持，自动换行继续保留。
    pub fn invalidate_layout(&mut self) {
        self.dirty = true;
    }

    /// 清空来源、记录、样式和累计计数，恢复创建时的配置。
    pub fn clear(&mut self) {
        *self = Self::new(self.defaults, self.encoding);
    }

    /// 返回是否重新准备。失败时保留上次结果和待解析来源，可修正服务后重试。
    pub fn prepare(
        &mut self,
        layout_settings: &LayoutSettings,
        timing_settings: &TimingSettings,
        mut measure: impl FnMut(char, u32) -> Result<i32>,
    ) -> Result<bool> {
        if self.encoding != layout_settings.encoding || self.encoding != timing_settings.encoding {
            return Err(Error::new(0, "消息页、布局和时序的编码路径不一致"));
        }
        if !self.dirty {
            return Ok(false);
        }
        let mut state = self.state;
        let mut incoming = Markup::parse_from(
            &self.source[self.parsed_bytes..],
            &self.defaults,
            &mut state,
        )
        .map_err(|mut error| {
            error.offset += self.parsed_bytes;
            error
        })?;
        if incoming.atoms.len() > read::MAX_ITEMS - self.markup.atoms.len() {
            return Err(Error::new(self.parsed_bytes, "消息页片段数量超过上限"));
        }
        for atom in &mut incoming.atoms {
            atom.source.start += self.parsed_bytes;
            atom.source.end += self.parsed_bytes;
        }
        let (times, accumulated_units) =
            incoming.atom_times(timing_settings, self.schedule.accumulated_units)?;
        let mut entries = incoming.entries(|_, _| Ok(0))?;
        if entries.len() > read::MAX_ITEMS - self.layout.entries.len() {
            return Err(Error::new(self.parsed_bytes, "消息布局条目数量超过上限"));
        }
        for entry in &mut entries {
            if let Some(atom) = &mut entry.atom {
                *atom += self.markup.atoms.len();
            }
        }
        let mut layout = self.layout.clone();
        layout.entries.extend(entries);
        for entry in &mut layout.entries {
            entry.display_anchor = None;
            if entry.glyph() {
                let AtomKind::Character(c) = entry.kind else {
                    unreachable!()
                };
                entry.size[0] = measure(c, entry.size[1] as u32)?;
            }
        }
        layout.arrange(layout_settings)?;
        let mut markup = self.markup.clone();
        markup.atoms.extend(incoming.atoms);
        markup.final_style = state.style;
        markup.restore_text_color = state.restore_color;
        let mut atom_times = self.atom_times.clone();
        atom_times.extend(times);
        let deadlines = markup.bind_times(&layout, &atom_times)?;
        for (entry, &deadline) in layout.entries.iter().zip(&deadlines) {
            if let Some(atom) = entry.atom {
                atom_times[atom] = Some(deadline);
            }
        }

        self.markup = markup;
        self.layout = layout;
        self.atom_times = atom_times;
        self.schedule = Schedule {
            deadlines,
            accumulated_units,
        };
        self.state = state;
        self.parsed_bytes = self.source.len();
        self.dirty = false;
        Ok(true)
    }
}
