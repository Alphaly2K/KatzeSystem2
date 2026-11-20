use katze_system2::{
    Result,
    host::{
        graphics::Rect,
        input::{ButtonSource, DeviceBinding, DeviceKeys, InputDevice, KeySample, PointerSample},
    },
    input::Region,
};
use std::{cell::Cell, ops::RangeInclusive, rc::Rc};

pub(crate) struct StoryReplay {
    automatic: RangeInclusive<u32>,
    advance_key: Option<RangeInclusive<u32>>,
    pointer: Rc<Cell<[i32; 2]>>,
    surface: [u32; 2],
    plan: Vec<usize>,
    previous: Vec<(u32, Rect)>,
    option: usize,
    step: usize,
}

impl StoryReplay {
    pub(super) fn from_env(surface: [u32; 2]) -> Option<Self> {
        let round = |name| std::env::var(name).ok()?.parse::<u32>().ok();
        let automatic =
            round("KATZE_PROBE_AUTO_FROM")?..=round("KATZE_PROBE_AUTO_UNTIL").unwrap_or(u32::MAX);
        let advance_key = round("KATZE_PROBE_CTRL_FROM")
            .map(|start| start..=round("KATZE_PROBE_CTRL_UNTIL").unwrap_or(u32::MAX));
        Some(Self {
            automatic,
            advance_key,
            pointer: Rc::new(Cell::new(replay_scale(surface, [900, 400]))),
            surface,
            plan: std::env::var("KATZE_PROBE_CHOICE_PLAN")
                .unwrap_or_default()
                .split(',')
                .filter_map(|item| item.trim().parse().ok())
                .collect(),
            previous: Vec::new(),
            option: 0,
            step: 0,
        })
    }

    pub(super) fn update(&mut self, frame: u32, regions: &[Region]) {
        if !self.automatic.contains(&frame) || !frame.is_multiple_of(4) {
            return;
        }
        let [minimum_width, minimum_height] = replay_scale(self.surface, [120, 15]);
        let [_, upper] = replay_scale(self.surface, [0, 60]);
        let [_, lower] = replay_scale(self.surface, [0, 600]);
        let mut choices: Vec<_> = regions
            .iter()
            .filter(|r| r.enabled() != 0 && r.selection_enabled() != 0)
            .map(|r| (r.id(), r.bounds()))
            .filter(|(_, r)| {
                r.left >= 0
                    && r.right <= self.surface[0] as i32
                    && r.top >= upper
                    && r.bottom < lower
                    && r.right >= r.left.saturating_add(minimum_width)
                    && r.bottom - r.top >= minimum_height
            })
            .collect();
        choices.sort_by_key(|(_, r)| (r.top, r.left));
        let [story_width, _] = replay_scale(self.surface, [900, 0]);
        let story_choice = is_story_choice(&choices, self.surface[0] as i32, story_width);
        if choices.len() > 1 && choices != self.previous {
            self.option = 0;
            if story_choice {
                self.option = self.plan.get(self.step).copied().unwrap_or(0);
                self.step += 1;
            }
            println!(
                "CHOICE_SELECTION frame={frame} option={} step={} count={} regions={choices:?}",
                self.option,
                self.step,
                choices.len()
            );
            self.previous.clone_from(&choices);
        } else if choices.is_empty() {
            self.previous.clear();
        }
        // 单独的消息窗工具按钮不代表剧情选择。
        let target = choices
            .get(self.option.min(choices.len().saturating_sub(1)))
            .filter(|_| choices.len() > 1 || story_choice);
        self.pointer.set(
            target.map_or(replay_scale(self.surface, [900, 400]), |(_, r)| {
                [(r.left + r.right) / 2, (r.top + r.bottom) / 2]
            }),
        );
    }
}

fn is_story_choice(choices: &[(u32, Rect)], surface_width: i32, wide: i32) -> bool {
    let Some(&(_, first)) = choices.first() else {
        return false;
    };
    if choices.iter().all(|(_, r)| r.right - r.left > wide) {
        return true;
    }
    if first.right - first.left < surface_width / 3 {
        return false;
    }
    if choices.len() == 1 {
        let center = first.left + (first.right - first.left) / 2;
        return center.abs_diff(surface_width / 2) <= (surface_width / 16) as u32;
    }
    choices.windows(2).all(|pair| {
        let [(_, a), (_, b)] = [pair[0], pair[1]];
        a.left == b.left && a.right == b.right && a.bottom <= b.top
    })
}

fn replay_scale(surface: [u32; 2], [x, y]: [i32; 2]) -> [i32; 2] {
    [x * surface[0] as i32 / 1280, y * surface[1] as i32 / 720]
}

/// 按轮次产生点击的设备；`KATZE_PROBE_CLICKS` 为逗号分隔的 `轮次@x:y` 项。
pub(crate) struct ScriptedDevice {
    polls: u32,
    clicks: Vec<(u32, [i32; 2])>,
    center: [i32; 2],
    keys: DeviceKeys,
    automatic: Option<RangeInclusive<u32>>,
    advance_key: Option<RangeInclusive<u32>>,
    replay_pointer: Option<Rc<Cell<[i32; 2]>>>,
}

impl ScriptedDevice {
    pub(crate) fn new(keys: DeviceKeys, replay: Option<&StoryReplay>, surface: [u32; 2]) -> Self {
        let center = surface.map(|value| (value / 2) as i32);
        let mut clicks: Vec<(u32, [i32; 2])> = std::env::var("KATZE_PROBE_CLICKS").map_or_else(
            |_| Vec::new(),
            |value| {
                value
                    .split(',')
                    .filter_map(|item| {
                        let (round, point) = if let Some((round, at)) = item.split_once('@') {
                            let (x, y) = at.split_once(':')?;
                            (round, [x.trim().parse().ok()?, y.trim().parse().ok()?])
                        } else {
                            (item, center)
                        };
                        Some((round.trim().parse().ok()?, point))
                    })
                    .collect()
            },
        );
        clicks.sort_by_key(|(frame, _)| *frame);
        Self {
            polls: 0,
            clicks,
            center,
            keys,
            automatic: replay.map(|replay| replay.automatic.clone()),
            advance_key: replay.and_then(|replay| replay.advance_key.clone()),
            replay_pointer: replay.map(|replay| replay.pointer.clone()),
        }
    }
}

impl InputDevice for ScriptedDevice {
    fn key_configuration(&self) -> Option<&[DeviceBinding; 80]> {
        Some(self.keys.bindings())
    }
    fn poll(&mut self) -> Result<()> {
        self.polls = self.polls.wrapping_add(1);
        let pressed =
            self.clicks
                .iter()
                .any(|&(round, _)| self.polls.wrapping_sub(round) < 2)
                || self.automatic.as_ref().is_some_and(|range| {
                    range.contains(&self.polls) && self.polls.is_multiple_of(4)
                });
        self.keys.button(ButtonSource::Mouse, 1, u32::from(pressed));
        if let Some(range) = &self.advance_key {
            self.keys.button(
                ButtonSource::Keyboard,
                0x11,
                u32::from(range.contains(&self.polls)),
            );
        }
        if std::env::var_os("KATZE_TRACE_INPUT").is_some() && self.polls < 320 {
            eprintln!("[input-poll] polls={}", self.polls);
        }
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        // 按下持续两轮，位置再保持两轮，使释放入轮也落在同一区域内。
        let lookahead = if self.replay_pointer.is_some() { 8 } else { 0 };
        let click = self.clicks.iter().find(|&&(round, _)| {
            self.polls.saturating_add(lookahead) >= round && self.polls < round.saturating_add(4)
        });
        Ok(PointerSample {
            position: click.map_or_else(
                || {
                    self.replay_pointer
                        .as_ref()
                        .map_or(self.center, |pointer| pointer.get())
                },
                |(_, position)| *position,
            ),
            active: 1,
        })
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        self.keys.sample(key, group, repeat)
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.keys.commit();
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn set_pointer(&mut self, _: [i32; 2]) -> Result<()> {
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        self.keys.clear_pressed();
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        Ok(())
    }
    fn cancel_wheel_repeat(&mut self) -> Result<()> {
        Ok(())
    }
}
