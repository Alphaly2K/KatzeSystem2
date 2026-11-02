use super::{AtomKind, Layout, Markup, TextEncoding, Wait};
use crate::{Error, Result, read};

/// 字间隔与句末间隔使用原始计数单位，time_scale 在生成时间时参与换算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingSettings {
    pub encoding: TextEncoding,
    /// None 对应尚无全局时间上下文；记录时间为零，普通字符不推进计数。
    pub time_scale: Option<u32>,
    pub character_delay: u32,
    pub period_delay: u32,
    pub skip_spaces: bool,
}

/// 与一份 Layout.entries 等长的原始显示时间；时间不保证单调递增。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub deadlines: Vec<u32>,
    /// 可传入后续片段，保留追加消息的计数；wr 会将其清零。
    pub accumulated_units: u32,
}

impl Markup {
    /// layout 必须来自本片段，注音沿其中的 display_anchor 继承正文时间。
    pub fn schedule(
        &self,
        layout: &Layout,
        settings: &TimingSettings,
        initial_units: u32,
    ) -> Result<Schedule> {
        if self.atoms.len() > read::MAX_ITEMS || layout.entries.len() > read::MAX_ITEMS {
            return Err(Error::new(0, "消息显示时序条目数量超过上限"));
        }
        let (times, accumulated_units) = self.atom_times(settings, initial_units)?;
        Ok(Schedule {
            deadlines: self.bind_times(layout, &times)?,
            accumulated_units,
        })
    }

    pub(super) fn atom_times(
        &self,
        settings: &TimingSettings,
        initial_units: u32,
    ) -> Result<(Vec<Option<u32>>, u32)> {
        let mut units = initial_units;
        let mut times = Vec::with_capacity(self.atoms.len());
        for atom in &self.atoms {
            match atom.kind {
                AtomKind::Wait(Wait::Frames(value)) => {
                    units = units.wrapping_add(value);
                    times.push(None);
                    continue;
                }
                AtomKind::Wait(Wait::Characters(value)) => {
                    let scale = settings.time_scale.ok_or_else(|| {
                        Error::new(atom.source.start, "消息 w 等待缺少全局时间上下文")
                    })?;
                    if scale as i32 > 0 {
                        units = settings
                            .character_delay
                            .wrapping_mul(value)
                            .wrapping_mul(10)
                            / scale;
                    }
                    times.push(None);
                    continue;
                }
                AtomKind::Wait(Wait::Reset) => units = 0,
                AtomKind::Character(c) if c as u32 > 0xffff => {
                    return Err(Error::new(
                        atom.source.start,
                        "消息非 BMP 字符的逐字时间映射尚未接入",
                    ));
                }
                _ => (),
            }
            if atom.group.is_some_and(|group| group.ruby)
                || matches!(atom.kind, AtomKind::X(value) if value > 2)
            {
                times.push(Some(0));
                continue;
            }
            let Some(scale) = settings.time_scale else {
                times.push(Some(0));
                continue;
            };
            times.push(Some(scale.wrapping_mul(units) / 10));
            if let AtomKind::Character(c) = atom.kind {
                let skip = c as u32 <= 9
                    || c == '\u{ffff}'
                    || (settings.skip_spaces && matches!(c, ' ' | '　'));
                if !skip {
                    let period =
                        c == '。' || (settings.encoding == TextEncoding::Unicode && c == '.');
                    units = units.wrapping_add(if period {
                        settings.period_delay
                    } else {
                        settings.character_delay
                    });
                }
            }
        }
        Ok((times, units))
    }

    pub(super) fn bind_times(&self, layout: &Layout, times: &[Option<u32>]) -> Result<Vec<u32>> {
        let mut deadlines = Vec::with_capacity(layout.entries.len());
        for (index, entry) in layout.entries.iter().enumerate() {
            let deadline = if let Some(atom) = entry.atom {
                if self
                    .atoms
                    .get(atom)
                    .is_none_or(|atom| atom.kind != entry.kind)
                {
                    return Err(Error::new(index, "消息布局的来源片段不匹配"));
                }
                times[atom].ok_or_else(|| Error::new(index, "消息布局引用了未登记的等待操作"))?
            } else if entry.kind == AtomKind::NewLine {
                0
            } else {
                return Err(Error::new(index, "消息布局的自动记录类别无效"));
            };
            let deadline = match entry.display_anchor {
                Some(anchor) if anchor < index => deadlines[anchor],
                Some(_) => {
                    return Err(Error::new(
                        index,
                        "消息显示时间的绑定目标必须位于当前记录之前",
                    ));
                }
                None => deadline,
            };
            deadlines.push(deadline);
        }
        Ok(deadlines)
    }
}
