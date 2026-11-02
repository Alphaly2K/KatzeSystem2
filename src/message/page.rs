use super::markup::ParseState;
use super::playback::Playback;
use super::{
    AtomKind, DisplayUpdate, GlyphEffects, GlyphMask, GlyphPaint, GlyphState, Layout, LayoutEntry,
    LayoutSettings, Markup, Schedule, Style, StyleDefaults, TextEncoding, TimingSettings,
};
use crate::{Error, Result, graphics::Surface, read};

/// 持有一页消息的来源、解析状态、布局及显示时间；追加文字在 prepare 时解析。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    defaults: StyleDefaults,
    pub(super) encoding: TextEncoding,
    source: String,
    characters: usize,
    parsed_bytes: usize,
    state: ParseState,
    markup: Markup,
    layout: Layout,
    atom_times: Vec<Option<u32>>,
    schedule: Schedule,
    pub(super) playback: Playback,
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
            playback: Playback::default(),
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

    pub fn speed(&self) -> u32 {
        self.playback.speed()
    }

    /// 十为普通速度；变化时以最近处理的时间和已显示记录时间为新基准。
    pub fn set_speed(&mut self, speed: u32) {
        self.playback.set_speed(speed);
    }

    /// 查询上次准备结果中的字形是否完整显示；控制记录为 false，越界为 None。
    pub fn is_visible(&self, layout_index: usize) -> Option<bool> {
        self.glyph_state(layout_index)
            .map(|state| state == GlyphState::Complete)
    }

    /// 控制记录返回 Hidden；渐显中的字形尚未完整显示。
    pub fn glyph_state(&self, layout_index: usize) -> Option<GlyphState> {
        self.layout.entries.get(layout_index).map(|entry| {
            if entry.glyph() {
                self.playback.states[entry.atom.unwrap()]
            } else {
                GlyphState::Hidden
            }
        })
    }

    /// 普通逐字显示；MAX 时刻强制显示，MAX 分隔上限表示扫描全部记录。
    pub fn advance(&mut self, now: u32, segment_limit: u32) -> Result<DisplayUpdate> {
        self.require_prepared()?;
        self.playback
            .advance(&self.layout, &self.schedule, now, segment_limit, |_| Ok(()))
    }

    pub(super) fn advance_painted(
        &mut self,
        now: u32,
        segment_limit: u32,
        mut draw: impl FnMut(&LayoutEntry, Style) -> Result<()>,
    ) -> Result<DisplayUpdate> {
        self.require_prepared()?;
        let layout = &self.layout;
        let markup = &self.markup;
        self.playback
            .advance(layout, &self.schedule, now, segment_limit, |index| {
                let entry = &layout.entries[index];
                draw(entry, markup.atoms[entry.atom.unwrap()].style)
            })
    }

    pub fn remaining(&self, segment_limit: u32) -> Result<u32> {
        self.require_prepared()?;
        Ok(self.playback.remaining(&self.layout, segment_limit))
    }

    /// 按登记顺序混合已显示的正文和注音前景；不清空目标，不生成阴影或渐显。
    pub fn paint_foreground(
        &self,
        target: &mut Surface,
        origin: [i32; 2],
        mut glyph: impl FnMut(char, u32) -> Result<GlyphMask>,
    ) -> Result<bool> {
        self.require_prepared()?;
        let mut changed = false;
        for entry in &self.layout.entries {
            if !entry.glyph() || self.playback.states[entry.atom.unwrap()] != GlyphState::Complete {
                continue;
            }
            let AtomKind::Character(character) = entry.kind else {
                unreachable!()
            };
            let style = self.markup.atoms[entry.atom.unwrap()].style;
            let mask = glyph(character, entry.size[1] as u32)?;
            let image = mask.colorize(
                if entry.group.is_some_and(|group| group.ruby) {
                    style.ruby_color
                } else {
                    style.text_color
                },
                false,
            )?;
            let at = std::array::from_fn(|i| {
                origin[i]
                    .wrapping_add(entry.position[i])
                    .wrapping_add(mask.offset()[i])
            });
            changed |= target.blend_from(&image, image.bounds(), at, 255);
        }
        Ok(changed)
    }

    /// 使用基线的一百二十八像素方形字形面，依登记顺序合成效果和前景。
    pub fn paint(
        &self,
        target: &mut Surface,
        origin: [i32; 2],
        effects: &GlyphEffects,
        double_coverage: bool,
        mut glyph: impl FnMut(char, u32) -> Result<GlyphMask>,
    ) -> Result<bool> {
        self.require_prepared()?;
        let mut changed = false;
        let mut canvas = Surface::new([128; 2])?;
        for entry in &self.layout.entries {
            if !entry.glyph() || self.playback.states[entry.atom.unwrap()] != GlyphState::Complete {
                continue;
            }
            let AtomKind::Character(character) = entry.kind else {
                unreachable!()
            };
            let style = self.markup.atoms[entry.atom.unwrap()].style;
            let mask = glyph(character, entry.size[1] as u32)?;
            if mask.coverage().is_empty() {
                continue;
            }
            canvas.clear(0);
            let bounds = mask.paint(
                &mut canvas,
                &GlyphPaint {
                    color: if entry.group.is_some_and(|group| group.ruby) {
                        style.ruby_color
                    } else {
                        style.text_color
                    },
                    shadow_color: style.shadow_color,
                    font_size: entry.size[1] as u32,
                    double_coverage,
                    effects: *effects,
                },
            )?;
            let at = std::array::from_fn(|i| origin[i].wrapping_add(entry.position[i]));
            changed |= target.blend_from(&canvas, bounds, at, 255);
        }
        Ok(changed)
    }

    pub(super) fn require_prepared(&self) -> Result<()> {
        if self.dirty {
            Err(Error::new(0, "消息页需要先完成准备"))
        } else {
            Ok(())
        }
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

    /// 清空来源、记录、样式和累计计数，恢复创建时的配置，保留显示速度。
    pub fn clear(&mut self) {
        let speed = self.playback.speed();
        *self = Self::new(self.defaults, self.encoding);
        self.playback.set_speed(speed);
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
        self.playback
            .states
            .resize(self.markup.atoms.len(), GlyphState::Hidden);
        self.parsed_bytes = self.source.len();
        self.dirty = false;
        Ok(true)
    }
}
