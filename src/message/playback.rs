use super::{AtomKind, Layout, Schedule};

/// 一轮普通逐字显示的结果；下标指向当前 Layout.entries。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayUpdate {
    pub newly_visible: Vec<usize>,
    /// 当前分隔范围内没有尚未到时的字形；已显示的字形仍参与时间检查。
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Playback {
    pub visible: Vec<bool>,
    speed: u32,
    latest_deadline: u32,
    last_time: u32,
    deadline_origin: u32,
    time_origin: u32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            visible: Vec::new(),
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

    pub fn advance(
        &mut self,
        layout: &Layout,
        schedule: &Schedule,
        now: u32,
        segment_limit: u32,
    ) -> DisplayUpdate {
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
                        return result;
                    }
                }
                continue;
            }
            let deadline = schedule.deadlines[index];
            if now != u32::MAX && deadline.wrapping_sub(self.deadline_origin) > elapsed {
                result.complete = false;
                continue;
            }
            let visible = &mut self.visible[entry.atom.unwrap()];
            if !*visible {
                *visible = true;
                result.newly_visible.push(index);
            }
            self.latest_deadline = self.latest_deadline.max(deadline);
        }
        self.last_time = now;
        result
    }

    pub fn remaining(&self, layout: &Layout, segment_limit: u32) -> u32 {
        let mut count = 0;
        let mut segments = 0i32;
        for entry in &layout.entries {
            if entry.glyph() {
                count += u32::from(!self.visible[entry.atom.unwrap()]);
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
