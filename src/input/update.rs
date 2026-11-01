use super::{
    CommandState, InputDelivery, InputDevice, InputState, KeyEventKind, KeySample, KeyServices,
    KeyTransition, Keys, PointerMotion, Regions,
};
use crate::kcs::Host;
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

impl InputState {
    pub fn update_device(&mut self, id: u32, host: &mut dyn Host) -> Result<()> {
        device(host)?.poll()?;
        let sample = device(host)?.pointer()?;
        self.commands.pointer.active = sample.active;
        if sample.position == self.commands.pointer.position
            && !self.commands.regions.geometry_dirty()
        {
            if sample.active != 0 {
                if device(host)?.is_foreground()? {
                    let p = &mut self.commands.pointer;
                    p.idle_count = p.idle_count.wrapping_add(1);
                    if p.auto_hide != 0
                        && p.cursor_requested != 0
                        && p.cursor_shown != 0
                        && p.idle_count >= p.idle_limit
                    {
                        self.commands.show_cursor(id, 0, true, host)?;
                    }
                }
            } else if self.commands.pointer.forced == 0 {
                self.commands.pointer.idle_count = 0;
                if self.cursor_needs_showing() {
                    self.commands.show_cursor(id, 1, false, host)?;
                }
            }
        } else {
            let point = if sample.active != 0 {
                Some(host.input_coordinates().to_logical(sample.position).0)
            } else {
                None
            };
            self.update_pointer_selection(id, point, host)?;
            let changed = sample.position != self.commands.pointer.position;
            if self.commands.pointer.forced == 0 && changed {
                self.commands.pointer.idle_count = 0;
                if self.cursor_needs_showing() {
                    self.commands.show_cursor(id, 1, true, host)?;
                }
            }
            self.commands.pointer.forced = 0;
        }
        self.commands.pointer.position = sample.position;
        let mut keys = std::mem::take(&mut self.commands.keys);
        let result = keys.scan(&mut DeviceScan {
            state: self,
            id,
            host,
        });
        self.commands.keys = keys;
        result?;
        if device(host)?.take_wheel()? != 0 {
            self.commands.pointer.origin = device(host)?.pointer()?.position;
            let point = host
                .input_coordinates()
                .to_logical(self.commands.pointer.position)
                .0;
            self.wheel(id, point, host)?;
        }
        let suppressed = self.events.suppressed();
        let first = device(host)?.key(0, 0, false)?.mask;
        let second = device(host)?.key(0, 1, false)?.mask;
        if (first | second) & 2 != 0 {
            let p = &self.commands.pointer;
            let delta: [u32; 2] =
                std::array::from_fn(|i| p.position[i].wrapping_sub(p.origin[i]).unsigned_abs());
            let moved = if suppressed == 0 {
                let moved = delta.into_iter().any(|d| d >= p.drag_threshold);
                if moved {
                    self.events.set_suppressed(1);
                }
                moved
            } else {
                delta != [0; 2]
            };
            if moved {
                let map = host.input_coordinates();
                let motion = PointerMotion {
                    code: 6,
                    previous: map.to_logical(self.commands.pointer.origin).0,
                    current: map.to_logical(self.commands.pointer.position).0,
                };
                self.dispatch_pointer_motion(id, motion, host)?;
                self.commands.pointer.origin = self.commands.pointer.position;
            }
        }
        device(host)?.finish()?;
        device(host)?.commit()?;
        self.commands.regions.clear_geometry_dirty();
        self.commands.pointer.history = device(host)?.history()?;
        Ok(())
    }

    fn cursor_needs_showing(&self) -> bool {
        let p = &self.commands.pointer;
        p.auto_hide != 0 && p.cursor_requested != 0 && p.cursor_shown == 0
    }

    fn wheel(&mut self, id: u32, point: [i32; 2], host: &mut dyn Host) -> Result<()> {
        let (regions, mut callback) = self.commands.split(self.events.raw_state());
        regions.dispatch_wheel(id, point, |regions, command| {
            callback
                .route(host, regions, command, &mut CommandBuffers::default())
                .map(|_| ())
        })
    }
}

impl CommandState {
    pub(super) fn show_cursor(
        &mut self,
        id: u32,
        visible: u32,
        notify: bool,
        host: &mut dyn Host,
    ) -> Result<()> {
        self.pointer.cursor_shown = visible;
        device(host)?.show_cursor(visible)?;
        if notify {
            host.dispatch_input_command(
                self,
                InputDelivery::Broadcast,
                Command {
                    sender: id,
                    target: 0,
                    code: 22,
                    data: [0, visible, 0],
                },
                &mut CommandBuffers::default(),
                &mut 1,
            )?;
        }
        Ok(())
    }
}

impl Regions {
    /// 滚轮通知忽略区域事件掩码，保留矩形与可选位图命中。
    pub fn dispatch_wheel(
        &mut self,
        sender: u32,
        point: [i32; 2],
        mut route: impl FnMut(&mut Self, Command) -> Result<()>,
    ) -> Result<()> {
        let mut index = 0;
        let mut visits = 0;
        while let Some(region) = self.records.get(index) {
            visits += 1;
            if visits > read::MAX_ITEMS {
                return Err(Error::new(0, "输入滚轮通知遍历超过上限"));
            }
            let rect = region.bounds();
            let [x, y] = point;
            if region.enabled() == 0
                || (rect.left < rect.right
                    && rect.top < rect.bottom
                    && (x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom))
                || region
                    .hit_mask()
                    .is_some_and(|mask| !mask.contains([rect.left, rect.top], point))
            {
                index += 1;
                continue;
            }
            let instance = region.instance;
            route(
                self,
                Command {
                    sender,
                    target: region.id(),
                    code: 5,
                    data: [0; 3],
                },
            )?;
            index = self.current(index, instance)?.0 + 1;
        }
        Ok(())
    }
}

struct DeviceScan<'a> {
    state: &'a mut InputState,
    id: u32,
    host: &'a mut dyn Host,
}

impl KeyServices for DeviceScan<'_> {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample> {
        device(self.host)?.key(key, group, true)
    }

    fn dispatch(&mut self, keys: &mut Keys, transition: KeyTransition) -> Result<bool> {
        std::mem::swap(keys, &mut self.state.commands.keys);
        let result = (|| {
            if transition.kind == KeyEventKind::Change {
                if transition.pressed != 0 {
                    self.state.commands.pointer.origin = device(self.host)?.pointer()?.position;
                }
                self.state.events.set_suppressed(0);
            }
            let point = if self.state.events.suppressed() == 0 {
                self.host
                    .input_coordinates()
                    .to_logical(self.state.commands.pointer.position)
                    .0
            } else {
                [0; 2]
            };
            self.state
                .dispatch_key(self.id, transition.event(point), self.host)
        })();
        std::mem::swap(keys, &mut self.state.commands.keys);
        result
    }
}

pub(super) fn device(host: &mut dyn Host) -> Result<&mut dyn InputDevice> {
    host.input_device()
        .ok_or_else(|| Error::new(0, "宿主未配置输入设备，自动设备更新不可用"))
}
