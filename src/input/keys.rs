use super::{Events, KeyEvent, KeyEventKind, Regions};
use crate::task::Command;
use crate::{Error, Result};

/// 一次设备查询的原始事件掩码和首次重复开关。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeySample {
    pub mask: u32,
    pub repeat: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyState {
    pub count: u32,
    pub mask: u32,
    pub consumed: bool,
}

/// group 只标识设备来源；转换为对象消息时不写入数据项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyTransition {
    pub group: u32,
    pub key: u32,
    pub kind: KeyEventKind,
    pub pressed: u32,
    pub mask: u32,
}

impl KeyTransition {
    pub(crate) fn event(self, point: [i32; 2]) -> KeyEvent {
        KeyEvent {
            kind: self.kind,
            code: 4,
            key: self.key,
            pressed: self.pressed,
            mask: self.mask,
            point,
        }
    }
}

/// 扫描期间即时查询设备并同步分派；消费结果决定是否继续扫描。
pub trait KeyServices {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample>;
    fn dispatch(&mut self, keys: &mut Keys, transition: KeyTransition) -> Result<bool>;
}

/// 四组、每组八十键的更新计数与消费状态。
pub struct Keys {
    records: [[KeyState; 4]; 80],
    enabled: [u32; 80],
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            records: [[KeyState::default(); 4]; 80],
            enabled: [1; 80],
        }
    }
}

impl Keys {
    /// 第一维为键槽，第二维为设备组。
    pub fn records(&self) -> &[[KeyState; 4]; 80] {
        &self.records
    }

    pub fn enabled(&self) -> &[u32; 80] {
        &self.enabled
    }

    pub(crate) fn clear_held(&mut self) {
        for (enabled, groups) in self.enabled.iter().zip(&mut self.records) {
            if *enabled != 0 {
                for state in groups {
                    if state.count != 0 {
                        *state = KeyState::default();
                    }
                }
            }
        }
    }

    /// 命令 151 修改单键开关，152 将全部开关恢复为一；结果槽不变。
    pub fn dispatch_command(&mut self, command: &Command, _output: &mut u32) -> Result<bool> {
        match command.code {
            151 => {
                let value = self
                    .enabled
                    .get_mut(command.data[0] as usize)
                    .ok_or_else(|| Error::new(0, "输入键槽位超出 0～79"))?;
                *value = command.data[1];
            }
            152 => self.enabled.fill(1),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// 按组再按键扫描；true 表示消费状态中断了本轮剩余扫描。
    /// 计数每轮加一，与帧增量和毫秒无关；重复阈值固定为 30／5。
    pub fn scan(&mut self, services: &mut impl KeyServices) -> Result<bool> {
        for group in 0..4 {
            for key in 0..80 {
                if self.enabled[key] == 0 {
                    continue;
                }
                let sample = services.sample(key as u32, group as u32)?;
                let state = &mut self.records[key][group];
                if sample.mask != 0 {
                    if sample.mask == state.mask && sample.mask != 16 {
                        state.count = state.count.wrapping_add(1);
                    } else {
                        state.mask = sample.mask;
                        state.count = 1;
                    }
                } else if state.count != 0 {
                    state.count = 0;
                    state.consumed = false;
                    let transition = transition(key, group, KeyEventKind::Change, 0, state.mask);
                    let consumed = services.dispatch(self, transition)?;
                    self.records[key][group].consumed = consumed;
                    self.records[key][group].mask = 0;
                    if consumed {
                        return Ok(true);
                    }
                    continue;
                }
                let state = self.records[key][group];
                if state.count == 0 {
                    continue;
                }
                let kind = if state.count == 1 {
                    Some(KeyEventKind::Change)
                } else if (state.count == 30 && sample.repeat != 0)
                    || (state.count & 0x8000_0000 != 0 && state.count & 0x7fff_ffff == 5)
                {
                    self.records[key][group].count = 0x8000_0000;
                    Some(KeyEventKind::Repeat)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    let transition = transition(key, group, kind, 1, state.mask);
                    let consumed = services.dispatch(self, transition)?;
                    self.records[key][group].consumed = consumed;
                    if consumed {
                        return Ok(true);
                    }
                } else if state.count & 0x8000_0000 == 0 && state.consumed {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

impl Events {
    /// 连接设备扫描产生的消息 4；按下和释放先解除分派抑制，重复保留它。
    pub fn dispatch_transition(
        &mut self,
        sender: u32,
        regions: &mut Regions,
        transition: KeyTransition,
        point: [i32; 2],
        route: impl FnMut(&mut Regions, Command) -> Result<Option<u32>>,
    ) -> Result<bool> {
        if transition.kind == KeyEventKind::Change {
            self.set_suppressed(0);
        }
        self.dispatch_key(sender, regions, transition.event(point), route)
    }
}

fn transition(
    key: usize,
    group: usize,
    kind: KeyEventKind,
    pressed: u32,
    mask: u32,
) -> KeyTransition {
    KeyTransition {
        key: key as u32,
        group: group as u32,
        kind,
        pressed,
        mask: if mask == 16 { 2 } else { mask },
    }
}
