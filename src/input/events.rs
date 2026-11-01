use super::{Region, Regions};
use crate::task::Command;
use crate::{Error, Result, read};

/// 每行按字节对齐、低位在先的命中位图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitMask {
    size: [u32; 2],
    bytes: Vec<u8>,
}

impl HitMask {
    pub fn new(size: [u32; 2], bytes: Vec<u8>) -> Result<Self> {
        let [width, height] = size;
        let length = (width as usize)
            .div_ceil(8)
            .checked_mul(height as usize)
            .filter(|&length| length <= read::MAX_BYTES);
        if width > i32::MAX as u32 - 7 || height > i32::MAX as u32 || length != Some(bytes.len()) {
            return Err(Error::new(0, "输入位掩码尺寸或字节长度无效"));
        }
        Ok(Self { size, bytes })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 点与原点相减保留 i32 回绕；负值及右下端点不命中。
    pub fn contains(&self, origin: [i32; 2], point: [i32; 2]) -> bool {
        let x = point[0].wrapping_sub(origin[0]);
        let y = point[1].wrapping_sub(origin[1]);
        if x < 0 || y < 0 || x >= self.size[0] as i32 || y >= self.size[1] as i32 {
            return false;
        }
        let index = y as usize * (self.size[0] as usize).div_ceil(8) + x as usize / 8;
        self.bytes[index] & (1 << (x % 8)) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEventKind {
    Change,
    Repeat,
}

/// point 为已转换的逻辑坐标；key 属于输入设备的 0～79 槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub kind: KeyEventKind,
    pub code: u32,
    pub key: u32,
    pub pressed: u32,
    pub mask: u32,
    pub point: [i32; 2],
}

/// 同步输入分派状态；设备采集和重复计时由调用者负责。
pub struct Events {
    captures: [u32; 80],
    raw_state: u32,
    suppressed: u32,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            captures: [0; 80],
            raw_state: 0,
            suppressed: 0,
        }
    }
}

impl Events {
    pub fn captures(&self) -> &[u32; 80] {
        &self.captures
    }

    pub fn raw_state(&self) -> u32 {
        self.raw_state
    }

    /// 输入命令 137 查询值。
    pub fn state(&self) -> u32 {
        if self.raw_state == 16 {
            2
        } else {
            self.raw_state
        }
    }

    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    pub fn set_suppressed(&mut self, value: u32) {
        self.suppressed = value;
    }

    /// None 表示目标不可达；Some(0) 消费事件，其他值继续。
    /// 回调可通过区域命令改变登记表，后续遍历读取变动后的顺序。
    pub fn dispatch_key(
        &mut self,
        sender: u32,
        regions: &mut Regions,
        event: KeyEvent,
        mut route: impl FnMut(&mut Regions, Command) -> Result<Option<u32>>,
    ) -> Result<bool> {
        self.dispatch_key_with_state(sender, regions, event, |_, regions, command| {
            route(regions, command)
        })
    }

    pub(crate) fn dispatch_key_with_state(
        &mut self,
        sender: u32,
        regions: &mut Regions,
        event: KeyEvent,
        mut route: impl FnMut(&mut Self, &mut Regions, Command) -> Result<Option<u32>>,
    ) -> Result<bool> {
        if self.suppressed != 0 {
            return Ok(false);
        }
        let key = event.key as usize;
        if key >= self.captures.len() {
            return Err(Error::new(0, "输入键槽位超出 0～79"));
        }
        self.raw_state = event.mask;
        if event.kind == KeyEventKind::Repeat {
            if event.pressed == 0 {
                return Ok(false);
            }
            let capture = self.captures[key];
            let candidate = if capture == 0 {
                regions.records.first()
            } else {
                regions.get(capture)
            };
            let Some(region) = candidate.filter(|region| accepts(region, event)) else {
                return Ok(false);
            };
            let command = packet(sender, region.id(), event);
            return Ok(route(self, regions, command)? == Some(0));
        }
        if event.pressed != 0 {
            self.captures[key] = 0;
        }
        let mut index = 0;
        let mut visits = 0;
        while let Some(region) = regions.records.get(index) {
            visits += 1;
            if visits > read::MAX_ITEMS {
                return Err(Error::new(0, "输入事件分派遍历超过上限"));
            }
            let eligible = if event.pressed != 0 {
                accepts(region, event)
            } else {
                region.enabled() != 0
                    && (self.captures[key] == 0 || region.id() == self.captures[key])
            };
            if !eligible {
                index += 1;
                continue;
            }
            let instance = region.instance;
            let command = packet(sender, region.id(), event);
            if route(self, regions, command)? == Some(0) {
                self.captures[key] = if event.pressed != 0 {
                    regions.current(index, instance)?.1.id()
                } else {
                    0
                };
                return Ok(true);
            }
            index = regions.current(index, instance)?.0 + 1;
        }
        if event.pressed == 0 {
            self.captures[key] = 0;
        }
        Ok(false)
    }
}

fn accepts(region: &Region, event: KeyEvent) -> bool {
    if region.enabled() == 0 || region.event_mask() & event.mask == 0 {
        return false;
    }
    if event.mask & 2 == 0 {
        return true;
    }
    let rect = region.bounds();
    let [x, y] = event.point;
    if rect.left < rect.right
        && rect.top < rect.bottom
        && (x < rect.left || y < rect.top || x >= rect.right || y >= rect.bottom)
    {
        return false;
    }
    region
        .hit_mask()
        .is_none_or(|mask| mask.contains([rect.left, rect.top], event.point))
}

fn packet(sender: u32, target: u32, event: KeyEvent) -> Command {
    Command {
        sender,
        target,
        code: event.code,
        data: [event.key, event.pressed, 0],
    }
}
