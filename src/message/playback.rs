use super::{AtomKind, Layout, Schedule};
use crate::Result;

/// 字形登记状态；Revealing 已写入完整来源面，尚未完成显示。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum GlyphState {
    #[default]
    Hidden = 0,
    Complete = 1,
    Revealing = 2,
}

/// 一轮普通逐字显示的结果；下标指向当前 Layout.entries。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayUpdate {
    pub newly_visible: Vec<usize>,
    /// 当前分隔范围内没有尚未到时的字形；已显示的字形仍参与时间检查。
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Playback {
    pub states: Vec<GlyphState>,
    speed: u32,
    latest_deadline: u32,
    last_time: u32,
    deadline_origin: u32,
    time_origin: u32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            states: Vec::new(),
            speed: 10,
            latest_deadline: 0,
            last_time: 0,
            deadline_origin: 0,
            time_origin: 0,
        }
    }
}

impl Playback {
    pub fn speed(&self) -> u32 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: u32) {
        if speed != self.speed {
            self.speed = speed;
            self.deadline_origin = self.latest_deadline;
            self.time_origin = self.last_time;
        }
    }

    pub fn finish_round(&mut self, now: u32) {
        self.last_time = now;
    }

    pub fn advance(
        &mut self,
        layout: &Layout,
        schedule: &Schedule,
        now: u32,
        segment_limit: u32,
        mut draw: impl FnMut(usize) -> Result<()>,
    ) -> Result<DisplayUpdate> {
        let elapsed = now.wrapping_sub(self.time_origin).wrapping_mul(self.speed) / 10;
        let mut segments = 0i32;
        let mut result = DisplayUpdate {
            newly_visible: Vec::new(),
            complete: true,
        };
        for (index, entry) in layout.entries.iter().enumerate() {
            if !entry.glyph() {
                if entry.kind == AtomKind::Character('\u{ffff}') && segment_limit != u32::MAX {
                    segments += 1;
                    if (segment_limit as i32) < segments {
                        return Ok(result);
                    }
                }
                continue;
            }
            let deadline = schedule.deadlines[index];
            if now != u32::MAX && deadline.wrapping_sub(self.deadline_origin) > elapsed {
                result.complete = false;
                continue;
            }
            let state = &mut self.states[entry.atom.unwrap()];
            if *state == GlyphState::Hidden {
                draw(index)?;
                *state = GlyphState::Complete;
                result.newly_visible.push(index);
            }
            self.latest_deadline = self.latest_deadline.max(deadline);
        }
        self.last_time = now;
        Ok(result)
    }

    pub fn remaining(&self, layout: &Layout, segment_limit: u32) -> u32 {
        let mut count = 0;
        let mut segments = 0i32;
        for entry in &layout.entries {
            if entry.glyph() {
                count += u32::from(self.states[entry.atom.unwrap()] != GlyphState::Complete);
            } else if entry.kind == AtomKind::Character('\u{ffff}') {
                if segment_limit != u32::MAX && (segment_limit as i32) <= segments {
                    break;
                }
                segments += 1;
            }
        }
        count
    }
}
