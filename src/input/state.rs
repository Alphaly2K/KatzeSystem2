use super::{
    Events, InputDevice, KeyEvent, KeyEventKind, KeySample, KeyServices, KeyTransition, Keys,
    PointerMotion, PointerState, Regions,
};
use crate::kcs::Host;
use crate::task::{Command, CommandBuffers, Control, TaskClock};
use crate::{Error, Result};

/// 同步回调期间由宿主持有的输入命令状态；同一份状态在嵌套命令间保持修改。
#[derive(Default)]
pub struct CommandState {
    pub control: Control,
    pub clock: TaskClock,
    pub regions: Regions,
    pub keys: Keys,
    pub pointer: PointerState,
    pub update_rate: u32,
    raw_state: u32,
    pub(super) suppressed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDelivery {
    Direct,
    Broadcast,
}

impl CommandState {
    /// 返回树的传播状态，已处理的输入或基类命令消费当前分支。
    pub fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<bool> {
        if self.control.deletion_requested != 0 {
            return Ok(false);
        }
        if self.regions.dispatch_command(command, buffers, output)?
            || self.keys.dispatch_command(command, output)?
        {
            return Ok(false);
        }
        if command.code == 137 {
            *output = if self.raw_state == 16 {
                2
            } else {
                self.raw_state
            };
            return Ok(false);
        }
        match command.code {
            3 => self.request_delete(host.input_device())?,
            126 => {
                let coordinates = host.input_coordinates();
                let point = coordinates
                    .to_logical(super::update::device(host)?.pointer()?.position)
                    .0;
                write(buffers.data0.as_deref_mut(), &point[0].to_le_bytes())?;
                write(buffers.data1.as_deref_mut(), &point[1].to_le_bytes())?;
            }
            138 | 139 => {
                if command.code == 138 && !host.input_pointer_control() {
                    return Ok(false);
                }
                if !super::update::device(host)?.is_foreground()? {
                    return Ok(false);
                }
                let packed = command.data[1];
                let point = [(packed & 0xffff) as i32, (packed >> 16) as i32];
                let (target, inside) = host.input_coordinates().to_device(point);
                if inside {
                    if self.suppressed != 0 {
                        self.keys.clear_held();
                        self.suppressed = 0;
                        super::update::device(host)?.clear_keys()?;
                    }
                    self.pointer.move_target = target;
                    self.pointer.move_remaining = command.data[0] as i32;
                    self.pointer.move_started = self.clock.elapsed();
                }
            }
            143 => {
                self.pointer.auto_hide = command.data[0];
                self.pointer.idle_limit = self.update_rate.wrapping_mul(command.data[1]) / 60;
                if self.pointer.auto_hide != 0 && self.pointer.cursor_requested != 0 {
                    self.show_cursor(command.target, 1, true, host)?;
                }
            }
            145 => {
                if command.data[0] >= 80 {
                    return Err(Error::new(0, "输入键槽位超出 0～79"));
                }
                for group in 0..4 {
                    let mask = super::update::device(host)?
                        .key(command.data[0], group, false)?
                        .mask;
                    if group == 0 {
                        *output = mask;
                    } else {
                        *output |= mask;
                    }
                }
            }
            146 => {
                for region in &mut self.regions.records {
                    region.words[7] = 0;
                }
                self.pointer.position = [-1; 2];
                self.pointer.forced = 1;
            }
            147 => self
                .regions
                .set_image_mask(command.data[0], command.data[1], host)?,
            149 => write(
                buffers.data1.as_deref_mut(),
                &self.pointer.cursor_shown.to_le_bytes(),
            )?,
            150 => {
                let device = super::update::device(host)?;
                device.show_cursor(command.data[1])?;
                self.pointer.cursor_requested = device.cursor_visible()?;
            }
            153 => *output = u32::from(self.pointer.active == 0),
            159 => {
                if self.pointer.auto_hide != 0 && self.pointer.cursor_requested != 0 {
                    self.show_cursor(command.target, 1, true, host)?;
                }
                self.pointer.idle_count = 0;
            }
            160 => {
                if buffers.data1.is_some() || command.data[1] != 0 {
                    write(buffers.data1.as_deref_mut(), &self.pointer.history)?;
                }
            }
            _ => {
                if (126..=160).contains(&command.code) {
                    return Err(Error::new(
                        0,
                        format!("输入对象命令 {} 尚未接入", command.code),
                    ));
                }
                return self.control.dispatch(
                    kind,
                    &mut self.clock,
                    command,
                    output,
                    |_, _, _, _| Ok(true),
                );
            }
        }
        Ok(false)
    }

    pub(crate) fn request_delete(&mut self, device: Option<&mut dyn InputDevice>) -> Result<()> {
        if self.control.deletion_requested == 0 {
            if let Some(device) = device {
                device.show_cursor(1)?;
            }
            self.control.deletion_requested = 1;
        }
        Ok(())
    }

    pub(super) fn split(&mut self, raw_state: u32) -> (&mut Regions, Callback<'_>) {
        self.raw_state = raw_state;
        let Self {
            control,
            clock,
            regions,
            keys,
            pointer,
            update_rate,
            raw_state,
            suppressed,
        } = self;
        (
            regions,
            Callback {
                control,
                clock,
                keys,
                pointer,
                update_rate,
                raw_state,
                suppressed,
            },
        )
    }
}

/// 输入任务持有的区域、键扫描、按键捕获和基类状态。
#[derive(Default)]
pub struct InputState {
    pub(crate) commands: CommandState,
    pub(super) events: Events,
}

impl InputState {
    pub fn regions(&self) -> &Regions {
        &self.commands.regions
    }
    pub fn regions_mut(&mut self) -> &mut Regions {
        &mut self.commands.regions
    }
    pub fn keys(&self) -> &Keys {
        &self.commands.keys
    }
    pub fn events(&self) -> &Events {
        &self.events
    }
    pub fn events_mut(&mut self) -> &mut Events {
        &mut self.events
    }
    pub fn pointer(&self) -> &PointerState {
        &self.commands.pointer
    }
    pub fn pointer_mut(&mut self) -> &mut PointerState {
        &mut self.commands.pointer
    }
    pub fn update_rate(&self) -> u32 {
        self.commands.update_rate
    }
    pub fn set_update_rate(&mut self, rate: u32) {
        self.commands.update_rate = rate;
    }
    pub(crate) fn initialize_pointer(&mut self, position: [i32; 2]) {
        self.commands.pointer.position = position;
        self.commands.pointer.active = 0;
    }

    pub fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<bool> {
        self.commands.raw_state = self.events.raw_state();
        self.commands.suppressed = self.events.suppressed();
        let result = self
            .commands
            .dispatch_command(kind, command, buffers, output, host);
        self.events.set_suppressed(self.commands.suppressed);
        result
    }

    pub fn dispatch_key(&mut self, id: u32, event: KeyEvent, host: &mut dyn Host) -> Result<bool> {
        let raw = if self.events.suppressed() == 0 && event.key < 80 {
            event.mask
        } else {
            self.events.raw_state()
        };
        let (regions, mut callback) = self.commands.split(raw);
        let result =
            self.events
                .dispatch_key_with_state(id, regions, event, |events, regions, command| {
                    callback.route(
                        host,
                        events,
                        regions,
                        command,
                        &mut CommandBuffers::default(),
                    )
                });
        self.commands.raw_state = self.events.raw_state();
        result
    }

    pub fn update_pointer_selection(
        &mut self,
        id: u32,
        point: Option<[i32; 2]>,
        host: &mut dyn Host,
    ) -> Result<()> {
        let (regions, mut callback) = self.commands.split(self.events.raw_state());
        regions.update_pointer_selection(id, point, |regions, command| {
            callback
                .route(
                    host,
                    &mut self.events,
                    regions,
                    command,
                    &mut CommandBuffers::default(),
                )
                .map(|_| ())
        })
    }

    pub fn dispatch_pointer_motion(
        &mut self,
        id: u32,
        motion: PointerMotion,
        host: &mut dyn Host,
    ) -> Result<()> {
        let (regions, mut callback) = self.commands.split(self.events.raw_state());
        regions.dispatch_pointer_motion(id, motion, |regions, command, buffers| {
            callback
                .route(host, &mut self.events, regions, command, buffers)
                .map(|_| ())
        })
    }

    pub fn scan_keys(
        &mut self,
        id: u32,
        point: [i32; 2],
        sample: &mut dyn FnMut(u32, u32) -> Result<KeySample>,
        host: &mut dyn Host,
    ) -> Result<bool> {
        let mut keys = std::mem::take(&mut self.commands.keys);
        let result = keys.scan(&mut Scan {
            state: self,
            id,
            point,
            sample,
            host,
        });
        self.commands.keys = keys;
        result
    }
}

pub(super) struct Callback<'a> {
    control: &'a mut Control,
    clock: &'a mut TaskClock,
    keys: &'a mut Keys,
    pointer: &'a mut PointerState,
    update_rate: &'a mut u32,
    raw_state: &'a mut u32,
    suppressed: &'a mut u32,
}

impl Callback<'_> {
    pub(super) fn route(
        &mut self,
        host: &mut dyn Host,
        events: &mut Events,
        regions: &mut Regions,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<Option<u32>> {
        let mut state = CommandState {
            control: std::mem::take(self.control),
            clock: std::mem::take(self.clock),
            regions: std::mem::take(regions),
            keys: std::mem::take(self.keys),
            pointer: std::mem::take(self.pointer),
            update_rate: *self.update_rate,
            raw_state: *self.raw_state,
            suppressed: events.suppressed(),
        };
        let mut output = 1;
        let result = host.dispatch_input_command(
            &mut state,
            InputDelivery::Direct,
            command,
            buffers,
            &mut output,
        );
        *self.control = state.control;
        *self.clock = state.clock;
        *regions = state.regions;
        *self.keys = state.keys;
        *self.pointer = state.pointer;
        *self.update_rate = state.update_rate;
        *self.raw_state = state.raw_state;
        *self.suppressed = state.suppressed;
        events.set_suppressed(state.suppressed);
        result.map(|found| found.then_some(output))
    }
}

fn write(buffer: Option<&mut [u8]>, value: &[u8]) -> Result<()> {
    let buffer = buffer
        .filter(|bytes| bytes.len() >= value.len())
        .ok_or_else(|| Error::new(0, "输入输出缓冲区缺失或过短"))?;
    buffer[..value.len()].copy_from_slice(value);
    Ok(())
}

struct Scan<'a> {
    state: &'a mut InputState,
    id: u32,
    point: [i32; 2],
    sample: &'a mut dyn FnMut(u32, u32) -> Result<KeySample>,
    host: &'a mut dyn Host,
}

impl KeyServices for Scan<'_> {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample> {
        (self.sample)(key, group)
    }

    fn dispatch(&mut self, keys: &mut Keys, transition: KeyTransition) -> Result<bool> {
        std::mem::swap(keys, &mut self.state.commands.keys);
        if transition.kind == KeyEventKind::Change {
            self.state.events.set_suppressed(0);
        }
        let result = self
            .state
            .dispatch_key(self.id, transition.event(self.point), self.host);
        std::mem::swap(keys, &mut self.state.commands.keys);
        result
    }
}
