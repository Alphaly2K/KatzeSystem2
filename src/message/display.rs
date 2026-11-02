use super::{
    AtomKind, GlyphEffects, GlyphMask, GlyphPaint, GlyphState, Layout, LayoutEntry, LayoutSettings,
    Page, Reveal, RevealGlyph, Style, StyleDefaults, TextEncoding, TimingSettings,
};
use crate::{
    Error, Result,
    graphics::{Rect, Surface},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplaySettings {
    pub effects: GlyphEffects,
    pub double_coverage: bool,
    /// 零使用普通逐字显示，非零使用水平渐显。
    pub reveal_width: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayProgress {
    /// 本轮首次绘制或预生成的 Layout.entries 下标。
    pub prepared: Vec<usize>,
    /// 本轮首次完整显示的 Layout.entries 下标。
    pub completed: Vec<usize>,
    pub complete: bool,
}

/// 一页消息及其持久像素；逐字和渐显更新共用同一份字形状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    page: Page,
    settings: DisplaySettings,
    surface: Surface,
    source: Option<Surface>,
    reveal: Option<Reveal>,
    starts: Vec<Option<i32>>,
    bounds: Vec<Rect>,
    normal_size: u32,
    character_delay: u32,
    colors_dirty: bool,
    dirty: Option<Rect>,
}

impl Display {
    pub fn new(
        defaults: StyleDefaults,
        encoding: TextEncoding,
        size: [u32; 2],
        settings: DisplaySettings,
    ) -> Result<Self> {
        let reveal = if settings.reveal_width == 0 {
            None
        } else {
            if size.contains(&0) {
                return Err(Error::new(0, "消息渐显需要非零尺寸的渲染面"));
            }
            Some(Reveal::new(settings.reveal_width)?)
        };
        let source = reveal.as_ref().map(|_| Surface::new(size)).transpose()?;
        Ok(Self {
            page: Page::new(defaults, encoding),
            settings,
            surface: Surface::new(size)?,
            source,
            reveal,
            starts: Vec::new(),
            bounds: Vec::new(),
            normal_size: defaults.text_sizes[2],
            character_delay: 0,
            colors_dirty: false,
            dirty: None,
        })
    }

    pub fn page(&self) -> &Page {
        &self.page
    }
    pub fn settings(&self) -> &DisplaySettings {
        &self.settings
    }
    pub fn surface(&self) -> &Surface {
        &self.surface
    }
    pub fn source_surface(&self) -> Option<&Surface> {
        self.source.as_ref()
    }
    /// 查询累计更新区域；读取后仍保留，直到显式清除。
    pub fn dirty_rectangle(&self) -> Option<Rect> {
        self.dirty
    }
    pub fn clear_dirty(&mut self) {
        self.dirty = None;
    }
    pub fn append(&mut self, text: &str) -> Result<()> {
        self.page.append(text)
    }
    pub fn invalidate_layout(&mut self) {
        self.page.invalidate_layout();
    }
    pub fn set_speed(&mut self, speed: u32) {
        self.page.set_speed(speed);
    }

    /// 从完整来源重新解析并准备；成功后替换配置、像素和进度，保留速度。
    pub fn reconfigure(
        &mut self,
        defaults: StyleDefaults,
        size: [u32; 2],
        settings: DisplaySettings,
        layout: &LayoutSettings,
        timing: &TimingSettings,
        measure: impl FnMut(char, u32) -> Result<i32>,
    ) -> Result<()> {
        let mut replacement = Self::new(defaults, self.page.encoding, size, settings)?;
        replacement.append(self.page.source())?;
        replacement.set_speed(self.page.speed());
        replacement.prepare(layout, timing, measure)?;
        *self = replacement;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.page.clear();
        self.clear_pixels();
        self.starts.clear();
        self.bounds.clear();
        self.colors_dirty = false;
    }

    /// 清空两份像素，保留来源、布局、字形进度与累计脏区。
    pub fn clear_pixels(&mut self) {
        self.surface.clear(0);
        if let Some(source) = &mut self.source {
            source.clear(0);
        }
    }

    pub fn prepare(
        &mut self,
        layout: &LayoutSettings,
        timing: &TimingSettings,
        measure: impl FnMut(char, u32) -> Result<i32>,
    ) -> Result<bool> {
        let changed = self.page.prepare(layout, timing, measure)?;
        if changed {
            self.bounds
                .resize(self.page.markup().atoms.len(), Rect::default());
            self.starts = self.page.layout().reveal_starts();
        }
        self.character_delay = timing.character_delay;
        Ok(changed)
    }

    pub fn advance(
        &mut self,
        now: u32,
        segment_limit: u32,
        mut glyph: impl FnMut(char, u32) -> Result<GlyphMask>,
    ) -> Result<DisplayProgress> {
        self.page.require_prepared()?;
        let mut canvas = Surface::new([128; 2])?;
        let encoding = self.page.encoding;
        let Some(reveal) = &self.reveal else {
            let settings = self.settings;
            let surface = &mut self.surface;
            let bounds = &mut self.bounds;
            let dirty = &mut self.dirty;
            let update = self
                .page
                .advance_painted(now, segment_limit, |entry, style| {
                    if let Some(drawn) = draw(
                        entry,
                        style,
                        surface,
                        &mut canvas,
                        &settings,
                        encoding,
                        &mut glyph,
                    )? {
                        bounds[entry.atom.unwrap()] = drawn.bounds;
                        if let Some(rect) = drawn.dirty {
                            record_dirty(dirty, rect, surface.bounds());
                        }
                    }
                    Ok(())
                })?;
            return Ok(DisplayProgress {
                prepared: update.newly_visible.clone(),
                completed: update.newly_visible,
                complete: update.complete,
            });
        };
        let end = segment_end(self.page.layout(), segment_limit);
        let mut progress = DisplayProgress {
            prepared: Vec::new(),
            completed: Vec::new(),
            complete: true,
        };
        let source = self.source.as_mut().unwrap();
        for index in 0..end {
            let entry = &self.page.layout().entries[index];
            if !entry.glyph() {
                continue;
            }
            let atom = entry.atom.unwrap();
            if self.page.playback.states[atom] != GlyphState::Hidden {
                continue;
            }
            let style = self.page.markup().atoms[atom].style;
            if let Some(drawn) = draw(
                entry,
                style,
                source,
                &mut canvas,
                &self.settings,
                encoding,
                &mut glyph,
            )? {
                self.bounds[atom] = drawn.bounds;
                if let Some(rect) = drawn.dirty {
                    record_dirty(&mut self.dirty, rect, source.bounds());
                }
            }
            self.page.playback.states[atom] = GlyphState::Revealing;
            self.colors_dirty = true;
            progress.prepared.push(index);
        }
        if self.colors_dirty {
            Reveal::copy_colors(&mut self.surface, source)?;
            self.colors_dirty = false;
        }
        let phase = Reveal::phase(now, self.normal_size, self.character_delay);
        for index in 0..end {
            let entry = &self.page.layout().entries[index];
            if !entry.glyph() {
                continue;
            }
            let atom = entry.atom.unwrap();
            match self.page.playback.states[atom] {
                GlyphState::Hidden => progress.complete = false,
                GlyphState::Complete => (),
                GlyphState::Revealing => {
                    let glyph = RevealGlyph {
                        position: entry.position,
                        bounds: self.bounds[atom],
                        start: self.starts[index].unwrap(),
                    };
                    let complete = reveal.apply(&mut self.surface, source, &glyph, phase)?;
                    if phase >= glyph.start {
                        record_dirty(
                            &mut self.dirty,
                            Rect {
                                left: glyph.position[0].wrapping_add(glyph.bounds.left),
                                top: glyph.position[1].wrapping_add(glyph.bounds.top),
                                right: glyph.position[0].wrapping_add(glyph.bounds.right),
                                bottom: glyph.position[1].wrapping_add(glyph.bounds.bottom),
                            },
                            self.surface.bounds(),
                        );
                    }
                    if complete {
                        self.page.playback.states[atom] = GlyphState::Complete;
                        progress.completed.push(index);
                    } else {
                        progress.complete = false;
                    }
                }
            }
        }
        if end == self.page.layout().entries.len() {
            self.page.playback.finish_round(now);
        }
        Ok(progress)
    }
}

fn segment_end(layout: &Layout, limit: u32) -> usize {
    let mut segments = 0i32;
    for (index, entry) in layout.entries.iter().enumerate() {
        if entry.kind == AtomKind::Character('\u{ffff}') && limit != u32::MAX {
            segments += 1;
            if (limit as i32) < segments {
                return index;
            }
        }
    }
    layout.entries.len()
}

#[derive(Default)]
struct DrawnGlyph {
    bounds: Rect,
    dirty: Option<Rect>,
}

fn draw(
    entry: &LayoutEntry,
    style: Style,
    target: &mut Surface,
    canvas: &mut Surface,
    settings: &DisplaySettings,
    encoding: TextEncoding,
    glyph: &mut impl FnMut(char, u32) -> Result<GlyphMask>,
) -> Result<Option<DrawnGlyph>> {
    if Rect::from_xywh(entry.position[0], entry.position[1], 128, 128)
        .intersect(target.bounds())
        .is_none()
    {
        return Ok(None);
    }
    let AtomKind::Character(character) = entry.kind else {
        unreachable!()
    };
    let mask = glyph(character, entry.size[1] as u32)?;
    if mask.coverage().is_empty() {
        return Ok(Some(DrawnGlyph::default()));
    }
    canvas.clear(0);
    let bounds = mask.paint(
        canvas,
        &GlyphPaint {
            color: if entry.group.is_some_and(|g| g.ruby) {
                style.ruby_color
            } else {
                style.text_color
            },
            shadow_color: style.shadow_color,
            font_size: entry.size[1] as u32,
            double_coverage: settings.double_coverage,
            effects: settings.effects,
        },
    )?;
    let mut source = bounds;
    let mut at = entry.position;
    if encoding == TextEncoding::Unicode && at[1] < 0 {
        // Unicode 保留原高度，将负 Y 的来源起点下移，再从目标顶部绘制。
        source.top = source.top.wrapping_sub(at[1]);
        source.bottom = source.bottom.wrapping_sub(at[1]);
        at[1] = 0;
    }
    Ok(Some(DrawnGlyph {
        bounds,
        dirty: target.blend_from_bounds(canvas, source, at, 255),
    }))
}

fn record_dirty(dirty: &mut Option<Rect>, rect: Rect, bounds: Rect) {
    if let Some(rect) = rect.intersect(bounds) {
        *dirty = Some(match *dirty {
            Some(previous) => Rect {
                left: previous.left.min(rect.left),
                top: previous.top.min(rect.top),
                right: previous.right.max(rect.right),
                bottom: previous.bottom.max(rect.bottom),
            },
            None => rect,
        });
    }
}
