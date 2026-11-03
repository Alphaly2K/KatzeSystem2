use super::{Drawable, Drawables, drawable::arithmetic};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 二维平面任务的基础生命周期、图元及绘图属性命令。
pub struct PlaneObject {
    id: u32,
    control: Control,
    clock: TaskClock,
    input_state: u32,
    input_locked: bool,
    input_states: [[u32; 3]; 4],
    event_target: u32,
    event_response: u32,
    hint_target: u32,
    input_configured: bool,
    display: u32,
    update_property: u32,
    blend_mode: u32,
    creation_flags: u32,
    offset: [f32; 2],
    size: [f32; 2],
    parent_offset: Option<[f32; 3]>,
    shape_kind: u32,
    geometry_name: Vec<u8>,
}

impl PlaneObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            control: Control::default(),
            clock: TaskClock::default(),
            input_state: 0,
            input_locked: false,
            input_states: [[0; 3]; 4],
            event_target: u32::MAX,
            event_response: 0,
            hint_target: u32::MAX,
            input_configured: false,
            display: 1,
            update_property: 1,
            blend_mode: 0,
            creation_flags: 1,
            offset: [0.0; 2],
            size: [0.0; 2],
            parent_offset: None,
            shape_kind: 0,
            geometry_name: Vec::new(),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn update_property(&self) -> u32 {
        self.update_property
    }

    pub fn hint_target(&self) -> u32 {
        self.hint_target
    }

    pub fn blend_mode(&self) -> u32 {
        self.blend_mode
    }

    pub fn creation_flags(&self) -> u32 {
        self.creation_flags
    }

    pub fn offset(&self) -> [f32; 2] {
        self.offset
    }

    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    pub fn parent_offset(&self) -> Option<[f32; 3]> {
        self.parent_offset
    }

    fn inherit_coordinates(&self, host: &mut dyn Host) -> Result<()> {
        let Some(offset) = self.parent_offset else {
            return Ok(());
        };
        let parent = host.parent_task_id()?;
        let mut position = [0; 12];
        let mut base = [0; 12];
        let mut output = 0;
        for (code, buffer) in [(62, &mut position), (61, &mut base)] {
            self.send_command(
                parent,
                code,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(buffer),
                    ..CommandBuffers::default()
                },
                &mut output,
                host,
            )?;
        }
        let base = words::<3>(Some(&base))?.map(f32::from_bits);
        let base = [
            arithmetic(2, base[0], offset[0]),
            arithmetic(2, base[1], offset[1]),
            arithmetic(1, offset[2], base[2]),
        ];
        let drawings = drawings(host)?;
        drawings.set_geometry_base_point(self.id, base, false);
        drawings.set_geometry_position(
            self.id,
            words::<3>(Some(&position))?.map(f32::from_bits),
            false,
        );
        Ok(())
    }

    pub fn shape_kind(&self) -> u32 {
        self.shape_kind
    }

    /// 上次成功装载的 KX2 名称；清除图元或装载失败不改写此值。
    pub fn geometry_name(&self) -> &[u8] {
        &self.geometry_name
    }

    fn input_task(&self, host: &mut dyn Host) -> Result<u32> {
        let mut input = 0;
        host.dispatch_task_command(
            Command {
                target: 0,
                sender: self.id,
                code: 34,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut input,
        )?;
        Ok(input)
    }

    fn input_command(
        &self,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
        host: &mut dyn Host,
    ) -> Result<u32> {
        let input = self.input_task(host)?;
        let mut output = 0;
        self.send_command(input, code, data, buffers, &mut output, host)?;
        Ok(output)
    }

    fn send_command(
        &self,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            buffers,
            output,
        )?;
        Ok(())
    }

    fn change_input_state(&mut self, state: u32, host: &mut dyn Host) -> Result<()> {
        let z = drawings(host)?
            .get(self.id)
            .and_then(|d| d.geometry_transform(true))
            .map_or(0., |t| t.position()[2]);
        if let Some(slot) = self.input_states.get(state as usize).filter(|v| v[0] != 0) {
            let drawings = drawings(host)?;
            drawings.set_geometry_position(
                self.id,
                [slot[1] as i32 as f32, slot[2] as i32 as f32, z],
                true,
            );
            if state == 3 && self.hint_target != u32::MAX {
                self.hide_hint(host)?;
            }
            self.input_state = state;
            if self.hint_target != u32::MAX {
                if state == 1 {
                    if self.update_property != 0 {
                        self.show_hint(host)?;
                    }
                } else {
                    self.hide_hint(host)?;
                }
            }
        }
        Ok(())
    }

    fn hide_hint(&self, host: &mut dyn Host) -> Result<()> {
        self.send_command(
            self.hint_target,
            47,
            [0; 3],
            &mut CommandBuffers::default(),
            &mut 0,
            host,
        )
    }

    fn show_hint(&self, host: &mut dyn Host) -> Result<()> {
        let input = self.input_task(host)?;
        let [surface_width, _] = host.surface_size();
        let mut rect = [0; 16];
        self.send_command(
            input,
            131,
            [self.id, 0, 0],
            &mut CommandBuffers {
                data1: Some(&mut rect),
                ..CommandBuffers::default()
            },
            &mut 0,
            host,
        )?;
        let mut width = [0; 4];
        let mut height = [0; 4];
        self.send_command(
            self.hint_target,
            75,
            [0; 3],
            &mut CommandBuffers {
                data0: Some(&mut width),
                data1: Some(&mut height),
            },
            &mut 0,
            host,
        )?;
        let position = hint_position(
            surface_width,
            words::<4>(Some(&rect))?,
            [f32::from_le_bytes(width), f32::from_le_bytes(height)],
        );
        let mut vector = [0; 12];
        write_words(Some(&mut vector), &position.map(f32::to_bits))?;
        self.send_command(
            self.hint_target,
            51,
            [0; 3],
            &mut CommandBuffers {
                data1: Some(&mut vector),
                ..CommandBuffers::default()
            },
            &mut 0,
            host,
        )?;
        self.send_command(
            self.hint_target,
            47,
            [0, 1, 0],
            &mut CommandBuffers::default(),
            &mut 0,
            host,
        )
    }

    fn select_input_state(&mut self, host: &mut dyn Host) -> Result<()> {
        let selected =
            self.input_command(134, [self.id, 0, 0], &mut CommandBuffers::default(), host)?;
        if selected != 0 && self.input_states[1][0] != 0 {
            self.change_input_state(1, host)?;
        } else if self.input_states[0][0] != 0 {
            self.change_input_state(0, host)?;
        }
        Ok(())
    }

    fn input_geometry(&self, host: &mut dyn Host) -> Result<([i32; 4], u32)> {
        let (base, position) = drawings(host)?
            .get(self.id)
            .and_then(|d| d.geometry_transform(false))
            .map(|t| (t.base_point(), t.position()))
            .ok_or_else(|| Error::new(0, "输入区域需要已创建的二维图元"))?;
        let origin: [f32; 2] = std::array::from_fn(|i| position[i] - base[i]);
        Ok((
            [
                truncate(origin[0]),
                truncate(origin[1]),
                truncate(origin[0] + self.size[0]),
                truncate(origin[1] + self.size[1]),
            ],
            truncate(base[2]) as u32,
        ))
    }

    fn refresh_input_rect(&self, host: &mut dyn Host) -> Result<()> {
        let input = self.input_task(host)?;
        if self.input_configured {
            let (rect, _) = self.input_geometry(host)?;
            let mut buffer = [0; 16];
            write_words(Some(&mut buffer), &rect.map(|value| value as u32))?;
            self.send_command(
                input,
                130,
                [self.id, 0, 0],
                &mut CommandBuffers {
                    data1: Some(&mut buffer),
                    ..CommandBuffers::default()
                },
                &mut 0,
                host,
            )?;
        }
        Ok(())
    }

    fn configure_input(&mut self, command: &Command, host: &mut dyn Host) -> Result<()> {
        if self.event_target == u32::MAX {
            let (mut rect, priority) = self.input_geometry(host)?;
            let logical = host.input_coordinates().logical_size;
            if rect[0] <= 0 && rect[1] <= 0 && rect[2] >= logical[0] && rect[3] >= logical[1] {
                rect = [-100_000, -100_000, 100_000, 100_000];
            }
            let mut region = [0; 60];
            write_words(
                Some(&mut region),
                &[
                    self.id,
                    priority,
                    7,
                    rect[0] as u32,
                    rect[1] as u32,
                    rect[2] as u32,
                    rect[3] as u32,
                ],
            )?;
            self.input_command(
                127,
                [priority, 0, 0],
                &mut CommandBuffers {
                    data1: Some(&mut region),
                    ..CommandBuffers::default()
                },
                host,
            )?;
            self.event_target = command.sender;
            self.event_response = 1;
        }
        self.input_configured = true;
        Ok(())
    }

    fn unregister_input(&mut self, propagation: u32, host: &mut dyn Host) -> Result<()> {
        if self.event_target != u32::MAX {
            self.input_command(
                128,
                [0, self.id, propagation],
                &mut CommandBuffers::default(),
                host,
            )?;
            self.event_target = u32::MAX;
            self.event_response = 1;
        }
        Ok(())
    }

    fn enable_input(&self, enabled: u32, propagation: u32, host: &mut dyn Host) -> Result<()> {
        if self.event_target != u32::MAX {
            let input = self.input_task(host)?;
            let mut output = 0;
            for code in [135].into_iter().chain((enabled == 0).then_some(133)) {
                self.send_command(
                    input,
                    code,
                    [self.id, enabled, propagation],
                    &mut CommandBuffers::default(),
                    &mut output,
                    host,
                )?;
            }
        }
        Ok(())
    }

    fn notify_event(
        &mut self,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
        host: &mut dyn Host,
    ) -> Result<()> {
        let mut output = 0;
        host.dispatch_graphics_event(
            &mut self.event_response,
            Command {
                target: self.event_target,
                sender: self.id,
                code,
                data,
            },
            buffers,
            &mut output,
        )?;
        Ok(())
    }

    fn forward(
        &self,
        code: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        let mut previous = None;
        let mut visits = 0;
        while let Some(target) = host.next_task_child(previous)? {
            visits += 1;
            if visits > crate::read::MAX_ITEMS {
                return Err(Error::new(0, "二维子任务传播超过上限"));
            }
            previous = Some(target);
            host.dispatch_task_command(
                Command {
                    target,
                    sender: self.id,
                    code,
                    data: command.data,
                },
                buffers,
                output,
            )?;
        }
        Ok(())
    }
}

impl TaskObject for PlaneObject {
    fn control(&self) -> &Control {
        &self.control
    }

    fn clock(&self) -> &TaskClock {
        &self.clock
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        (&mut self.control, &mut self.clock)
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        if context.is_empty() {
            Ok(())
        } else {
            Err(Error::new(0, "二维对象创建上下文尚未接入"))
        }
    }

    fn update_body(
        &mut self,
        _host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        // 运动与历史网格只能由尚未接入的专属命令创建；基础平面主体不推进脚本。
        self.event_response = 0;
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }

    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        _budget: usize,
    ) -> Result<CommandOutcome> {
        if self.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let id = self.id;
        match command.code {
            3 | 68 => {
                drawings(host)?.remove(id);
                if command.code == 68 {
                    self.size = [0.0; 2];
                }
                if command.code == 3 {
                    self.unregister_input(command.data[2], host)?;
                    self.control.deletion_requested = 1;
                }
            }
            4 => {
                if self.input_state == 3 {
                    return Ok(CommandOutcome::finished(false));
                }
                let raw = self.input_command(
                    137,
                    [id, 0, command.data[2]],
                    &mut CommandBuffers::default(),
                    host,
                )?;
                if raw & 2 != 0 {
                    if self.input_locked {
                        return Ok(CommandOutcome::finished(false));
                    }
                    if command.data[1] != 0 {
                        self.change_input_state(2, host)?;
                    } else {
                        self.select_input_state(host)?;
                    }
                }
                *output = 1;
                self.notify_event(
                    if command.data[1] != 0 { 9 } else { 10 },
                    [id, command.data[0], 0],
                    &mut CommandBuffers::default(),
                    host,
                )?;
                *output = self.event_response;
                self.event_response = 0;
            }
            5 | 6 => {
                *output = 1;
                let mut empty = CommandBuffers::default();
                self.notify_event(
                    if command.code == 5 { 65 } else { 6 },
                    if command.code == 5 {
                        [id, command.data[0], 0]
                    } else {
                        [command.data[0], command.data[1], 0]
                    },
                    if command.code == 6 {
                        buffers
                    } else {
                        &mut empty
                    },
                    host,
                )?;
                *output = self.event_response;
                self.event_response = 0;
            }
            7 | 8 => {
                if self.input_state != 3 && !self.input_locked {
                    let selected = u32::from(command.code == 7);
                    self.change_input_state(selected, host)?;
                    self.notify_event(64, [id, selected, 0], &mut CommandBuffers::default(), host)?;
                }
            }
            41 | 42 | 45 => {
                drawings(host)?.remove(id);
                let drawable = if command.code != 45 {
                    let words = words::<8>(buffers.data1.as_deref())?;
                    Drawable::rectangle(
                        [
                            f32::from_bits(words[0]),
                            f32::from_bits(words[1]),
                            f32::from_bits(words[2]),
                        ],
                        [f32::from_bits(words[3]), f32::from_bits(words[4])],
                        if command.code == 41 {
                            u32::MAX
                        } else {
                            words[7]
                        },
                    )
                } else {
                    Drawable::default()
                };
                if self.creation_flags == 1 {
                    let drawable = if command.code == 41 {
                        let layout = host
                            .canvas_layout()?
                            .ok_or_else(|| Error::new(0, "宿主未提供画布纹理布局"))?;
                        Drawable::from_canvas_source(drawable, layout)?
                    } else {
                        drawable
                    };
                    let bounds = drawable.bounds();
                    self.offset = [0.0; 2];
                    self.size =
                        std::array::from_fn(|i| arithmetic(2, bounds[1][i], bounds[0][i]).abs());
                    self.shape_kind = match command.code {
                        41 => 1,
                        42 => 2,
                        _ => 5,
                    };
                    drawings(host)?.insert(id, drawable)?;
                }
            }
            70 => {
                let name = crate::read::zstr(
                    buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "对象命令输入缓冲区缺失"))?,
                    0,
                )?;
                if name.len() >= 260 {
                    return Err(Error::new(0, "几何资源名超过基线缓冲区范围"));
                }
                drawings(host)?.remove(id);
                if self.creation_flags == 1 {
                    let services = host
                        .resource_services()
                        .ok_or_else(|| Error::new(0, "宿主未提供资源服务"))?;
                    if let Some(data) =
                        services.read(name, crate::resource::ResourceMode::LooseFirst)?
                    {
                        if data.get(..4) == Some(b"KX2\0") {
                            let drawable = Drawable::from_kx2(&data)?;
                            let bounds = drawable.bounds();
                            self.offset = [bounds[0][0], bounds[0][1]];
                            self.size = std::array::from_fn(|i| {
                                arithmetic(2, bounds[1][i], bounds[0][i]).abs()
                            });
                            self.shape_kind = 7;
                            self.geometry_name = name.to_vec();
                            drawings(host)?.insert(id, drawable)?;
                        }
                    }
                }
            }
            83 => {
                drawings(host)?.remove(id);
                let drawable = Drawable::from_canvas(
                    buffers.data1.as_deref().unwrap_or(&[]),
                    command.data[0],
                )?;
                drawings(host)?.insert(id, drawable)?;
            }
            49 | 51 | 55 | 66 => {
                let source = command.data[0] != 0;
                let next =
                    if matches!(command.code, 49 | 51) && !source && self.parent_offset.is_some() {
                        self.inherit_coordinates(host)?;
                        command.code + 1
                    } else {
                        let value = words::<3>(buffers.data1.as_deref())?.map(f32::from_bits);
                        let drawings = drawings(host)?;
                        match command.code {
                            49 => {
                                drawings.set_geometry_base_point(id, value, source);
                                50
                            }
                            51 => {
                                drawings.set_geometry_position(id, value, source);
                                52
                            }
                            55 => {
                                drawings.set_geometry_rotation(id, value, source);
                                56
                            }
                            _ => {
                                drawings.set_scale(id, value);
                                67
                            }
                        }
                    };
                self.refresh_input_rect(host)?;
                self.forward(next, command, buffers, output, host)?;
            }
            50 | 52 | 56 | 58 | 67 => {
                if self.parent_offset.is_some() {
                    match command.code {
                        50 | 52 if command.data[0] == 0 => self.inherit_coordinates(host)?,
                        56 | 67 => {
                            let value = words::<3>(buffers.data1.as_deref())?.map(f32::from_bits);
                            let drawings = drawings(host)?;
                            if command.code == 56 {
                                drawings.set_geometry_rotation(id, value, command.data[0] != 0);
                            } else {
                                drawings.set_scale(id, value);
                            }
                        }
                        58 => {
                            let value = words::<16>(buffers.data1.as_deref())?.map(f32::from_bits);
                            drawings(host)?.set_geometry_matrix(id, value, command.data[0] != 0);
                        }
                        _ => {}
                    }
                }
                if command.code != 52 || command.data[0] == 0 {
                    self.refresh_input_rect(host)?;
                }
                self.forward(command.code, command, buffers, output, host)?;
            }
            53 => {
                self.parent_offset =
                    Some(words::<3>(buffers.data1.as_deref())?.map(f32::from_bits));
                self.inherit_coordinates(host)?;
            }
            54 => self.parent_offset = None,
            47 | 48 => {
                if command.code == 47 {
                    self.display = command.data[1];
                }
                let inherited = command.code == 47 && self.parent_offset.is_some();
                let incoming = if inherited {
                    let parent = host.parent_task_id()?;
                    drawings(host)?.get(parent).map(Drawable::display)
                } else {
                    Some(command.data[1])
                };
                if let Some(incoming) = incoming {
                    let effective = if self.display == 0 { 0 } else { incoming };
                    drawings(host)?.set_display(id, effective);
                    if (command.code == 48 || inherited) && self.input_configured {
                        self.enable_input(effective, 0, host)?;
                    }
                }
                if self.input_state != 3 {
                    self.select_input_state(host)?;
                }
                if command.data[1] == 0 && self.hint_target != u32::MAX {
                    self.hide_hint(host)?;
                }
                self.forward(48, command, buffers, output, host)?;
            }
            61 | 62 => {
                query_geometry(drawings(host)?, id, command, buffers)?;
            }
            59 => {
                drawings(host)?.set_color(id, command.data[1], command.data[0]);
            }
            63 => {
                drawings(host)?.set_alpha(id, command.data[1]);
            }
            72 | 90 => {
                self.configure_input(command, host)?;
                if command.code == 72 {
                    let values = words::<12>(buffers.data0.as_deref())?;
                    self.input_states =
                        std::array::from_fn(|i| values[i * 3..i * 3 + 3].try_into().unwrap());
                    if self.input_states[0][0] != 0 {
                        self.change_input_state(0, host)?;
                    }
                }
            }
            73 => {
                self.change_input_state(command.data[1] & 0xffff, host)?;
                self.input_locked = command.data[1] >> 31 != 0;
            }
            74 => *output = self.input_state | if self.input_locked { 0x8000_0000 } else { 0 },
            91 => {
                self.input_states = [[0; 3]; 4];
                self.unregister_input(command.data[2], host)?;
            }
            93 => self.enable_input(command.data[1], command.data[2], host)?,
            95 => self.event_response = command.data[1],
            96 => {
                *output = self.input_command(
                    137,
                    [id, 0, command.data[2]],
                    &mut CommandBuffers::default(),
                    host,
                )?;
            }
            97 => self.event_target = command.data[1],
            98 => {
                self.input_command(
                    147,
                    [id, command.data[1], command.data[2]],
                    &mut CommandBuffers::default(),
                    host,
                )?;
            }
            75 => {
                write_words(buffers.data0.as_deref_mut(), &[self.size[0].to_bits()])?;
                write_words(buffers.data1.as_deref_mut(), &[self.size[1].to_bits()])?;
            }
            76 => {
                write_words(
                    buffers.data0.as_deref_mut(),
                    &[self.offset[0].to_bits(), self.offset[1].to_bits(), 0],
                )?;
                write_words(
                    buffers.data1.as_deref_mut(),
                    &[
                        arithmetic(1, self.size[0], self.offset[0]).to_bits(),
                        arithmetic(1, self.size[1], self.offset[1]).to_bits(),
                        0,
                    ],
                )?;
            }
            77 => {
                self.hint_target = command.data[1];
                self.hide_hint(host)?;
            }
            78 => self.update_property = command.data[1],
            81 => {
                let value = command.data[1];
                self.blend_mode = if value <= 5 { value } else { 0 };
                drawings(host)?.set_blend_mode(id, value);
            }
            84 => self.creation_flags = command.data[1],
            57 => {
                let value = words::<16>(buffers.data1.as_deref())?.map(f32::from_bits);
                drawings(host)?.set_geometry_matrix(id, value, command.data[0] != 0);
                self.refresh_input_rect(host)?;
                self.forward(58, command, buffers, output, host)?;
            }
            60 | 71 | 85 | 86 => {}
            41..=99 | 591 | 592 => {
                return Err(Error::new(
                    0,
                    format!("二维对象命令 {} 尚未接入", command.code),
                ));
            }
            _ => {
                let propagate = self.control.dispatch(
                    kind,
                    &mut self.clock,
                    command,
                    output,
                    |_, _, _, _| Ok(true),
                )?;
                return Ok(CommandOutcome::finished(propagate));
            }
        }
        Ok(CommandOutcome::finished(false))
    }

    fn enqueue_message(&mut self, _message: [u32; 4]) -> Result<()> {
        Err(Error::new(0, "二维对象不接收普通 KCS 队列消息"))
    }

    fn dispatch_message(
        &mut self,
        _message: &mut [u32; 4],
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<bool> {
        Err(Error::new(0, "二维对象不接收普通 KCS 同步消息"))
    }

    fn request_delete(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        drawings(host)?.remove(self.id);
        self.unregister_input(0, host)?;
        self.control.deletion_requested = 1;
        Ok(true)
    }
}

fn hint_position(surface_width: u32, rect: [u32; 4], size: [f32; 2]) -> [f32; 3] {
    // 原尺寸转换返回 i64，定位只使用其低 DWORD；无效转换的低位为零。
    let [width, height] = size.map(|value| {
        if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&(value as f64)) {
            value as i64 as u32
        } else {
            0
        }
    });
    let above = rect[1].wrapping_sub(height);
    let y = if (above as i32) >= 0 { above } else { rect[3] };
    let half = rect[0].wrapping_sub(rect[2]).wrapping_add(width) >> 1;
    let mut x = rect[0].wrapping_sub(half);
    if x.wrapping_add(width) >= surface_width {
        x = surface_width.wrapping_sub(width);
    }
    [(x as i32).max(0) as f32, y as i32 as f32, 0.]
}

fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn words<const N: usize>(buffer: Option<&[u8]>) -> Result<[u32; N]> {
    let bytes = buffer
        .filter(|b| b.len() >= N * 4)
        .ok_or_else(|| Error::new(0, "对象命令输入缓冲区缺失或过短"))?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

pub(crate) fn query_geometry(
    drawings: &Drawables,
    id: u32,
    command: &Command,
    buffers: &mut CommandBuffers<'_>,
) -> Result<()> {
    if let Some(transform) = drawings
        .get(id)
        .and_then(|d| d.geometry_transform(command.data[0] != 0))
    {
        let value = if command.code == 61 {
            transform.base_point()
        } else {
            transform.position()
        };
        write_words(buffers.data1.as_deref_mut(), &value.map(f32::to_bits))?;
    }
    Ok(())
}

fn write_words(buffer: Option<&mut [u8]>, words: &[u32]) -> Result<()> {
    let bytes = buffer
        .filter(|b| b.len() >= words.len() * 4)
        .ok_or_else(|| Error::new(0, "对象命令输出缓冲区缺失或过短"))?;
    for (value, bytes) in words.iter().zip(bytes.chunks_exact_mut(4)) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn drawings(host: &mut dyn Host) -> Result<&mut Drawables> {
    host.drawables()
        .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))
}

/// 创建二维平面、图像列表和画布淡入淡出任务，其余类型委托已有工厂。
pub struct GraphicsFactory<F> {
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for GraphicsFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        match object_kind {
            kind::PLANE => Ok(Box::new(PlaneObject::new(id))),
            kind::IMAGE_LIST => Ok(Box::new(super::ImageListObject::new(id))),
            kind::FADE => Ok(Box::new(super::FadeObject::new())),
            _ => self.fallback.create(id, object_kind, name),
        }
    }
}
