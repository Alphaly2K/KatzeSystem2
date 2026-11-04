use super::SliderControl;
use crate::graphics::{ImageList, Rect};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

#[derive(Default)]
struct Configuration {
    list: u32,
    priority: u32,
    rectangle: Rect,
    offset: [i32; 2],
}

/// 类型 42 的滑块任务，持有来源帧和轨道／滑块按钮。
pub struct SliderObject {
    id: u32,
    base: BaseObject,
    config: Configuration,
    control: SliderControl,
    frames: BTreeMap<u32, u32>,
    selected: Option<u32>,
    buttons: [Option<u32>; 2],
    geometry: [i32; 6],
    vertical: i32,
    loaded: bool,
    alpha: u32,
    states: [u32; 4],
    current_state: i32,
    inside: i32,
    drag_origin: [i32; 2],
    drag_pixel: i32,
    properties: [u32; 15],
}

impl SliderObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            config: Configuration::default(),
            control: SliderControl::default(),
            frames: BTreeMap::new(),
            selected: None,
            buttons: [None; 2],
            geometry: [0; 6],
            vertical: 0,
            loaded: false,
            alpha: 255,
            states: [0, 0, 2, 3],
            current_state: 0,
            inside: 0,
            drag_origin: [0; 2],
            drag_pixel: 0,
            properties: [0; 15],
        }
    }

    pub fn values(&self) -> &SliderControl {
        &self.control
    }

    /// 顺序为轨道、滑块；资源缺失或创建失败时可为空。
    pub fn buttons(&self) -> [Option<u32>; 2] {
        self.buttons
    }

    pub fn direction(&self) -> i32 {
        self.vertical
    }

    pub fn current_state(&self) -> i32 {
        self.current_state
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn alpha(&self) -> u32 {
        self.alpha
    }

    fn send(
        &self,
        host: &mut dyn Host,
        target: u32,
        code: u32,
        data: [u32; 3],
        data0: Option<&mut [u8]>,
        data1: Option<&mut [u8]>,
    ) -> Result<u32> {
        let mut output = 0;
        host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            &mut CommandBuffers {
                data0,
                data1,
                external: None,
            },
            &mut output,
        )?;
        Ok(output)
    }

    fn input(&self, host: &mut dyn Host) -> Result<u32> {
        self.send(host, 0, 34, [0; 3], None, None)
    }

    fn pointer(&self, input: u32, host: &mut dyn Host) -> Result<[i32; 2]> {
        let mut x = [0; 4];
        let mut y = [0; 4];
        self.send(host, input, 126, [0; 3], Some(&mut x), Some(&mut y))?;
        Ok([i32::from_le_bytes(x), i32::from_le_bytes(y)])
    }

    fn clear_source(&mut self, host: &mut dyn Host) -> Result<()> {
        self.send(
            host,
            self.config.list,
            287,
            [self.config.priority, 0, 0],
            None,
            None,
        )?;
        while let Some((&frame, &image)) = self.frames.first_key_value() {
            images(host)?.release(image)?;
            self.frames.remove(&frame);
        }
        self.selected = None;
        Ok(())
    }

    fn load(&mut self, bytes: &[u8], host: &mut dyn Host) -> Result<()> {
        read::range(bytes, 0, 0x124)?;
        self.clear_source(host)?;
        let name = &bytes[4..0x108];
        let end = name
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::new(4, "滑块资源名称缺少结尾零"))?;
        self.config = Configuration {
            list: read::u32_at(bytes, 0)?,
            priority: read::u32_at(bytes, 0x108)?,
            rectangle: rect(&bytes[0x10c..])?,
            offset: [
                read::u32_at(bytes, 0x11c)? as i32,
                read::u32_at(bytes, 0x120)? as i32,
            ],
        };
        self.loaded = false;
        let data = host
            .resource_services()
            .ok_or_else(|| Error::new(0, "宿主未提供滑块资源服务"))?
            .read(&name[..end], crate::resource::ResourceMode::LooseFirst)?;
        let Some(data) = data.filter(|data| !data.is_empty()) else {
            return Ok(());
        };
        let decoded = ImageList::decode_hg3(&data)?;
        let image = decoded
            .get(&0)
            .ok_or_else(|| Error::new(0, "滑块资源缺少帧零"))?;
        let info = *image.info();
        self.geometry = [
            info.offset[0],
            info.offset[1],
            info.size[0] as i32,
            info.size[1] as i32,
            info.canvas_size[0] as i32,
            info.canvas_size[1] as i32,
        ];
        for (frame, image) in decoded {
            let alpha = Some(image.info().reserved != 0);
            let handle = images(host)?.insert(self.id, image, alpha)?;
            self.frames.insert(frame, handle);
        }
        self.selected = self.frames.get(&0).copied();
        let r = self.config.rectangle;
        let width = r.right.wrapping_sub(r.left);
        let height = r.bottom.wrapping_sub(r.top);
        self.vertical = i32::from((width as u32) < (height as u32));
        self.control.travel = [
            0,
            if self.vertical == 0 {
                width.wrapping_sub(self.geometry[4])
            } else {
                height.wrapping_sub(self.geometry[5])
            },
        ];
        self.properties = [0; 15];
        self.properties[0] = images(host)?.pixel_handle(self.selected.unwrap())?;
        self.properties[4] = (r.left.wrapping_add(self.config.offset[0]) as f32).to_bits();
        self.properties[5] = (r.top.wrapping_add(self.config.offset[1]) as f32).to_bits();
        self.properties[9] = 1f32.to_bits();
        self.properties[10] = 1f32.to_bits();
        self.properties[11] = self.alpha;
        self.properties[12] = 1;
        self.submit(286, host)?;
        for i in 0..2 {
            if self.buttons[i].is_none() {
                self.buttons[i] = Some(host.create_task_child(kind::BUTTON, b"slider btn", b"")?);
            }
        }
        self.register_button(0, r, self.config.priority, host)?;
        let left = r.left.wrapping_add(info.offset[0]);
        let top = r.top.wrapping_add(info.offset[1]);
        self.register_button(
            1,
            Rect::from_xywh(left, top, info.size[0] as i32, info.size[1] as i32),
            self.config.priority.wrapping_add(1),
            host,
        )?;
        self.loaded = true;
        Ok(())
    }

    fn register_button(
        &self,
        index: usize,
        rectangle: Rect,
        priority: u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        let Some(task) = self.buttons[index] else {
            return Ok(());
        };
        let mut bytes = [
            rectangle.left as u32,
            rectangle.top as u32,
            rectangle.right as u32,
            rectangle.bottom as u32,
            priority,
            task,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
        self.send(host, task, 90, [0; 3], None, Some(&mut bytes))?;
        Ok(())
    }

    fn submit(&self, code: u32, host: &mut dyn Host) -> Result<()> {
        let mut bytes = self
            .properties
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.send(
            host,
            self.config.list,
            code,
            [self.config.priority, 0, 0],
            None,
            Some(&mut bytes),
        )?;
        Ok(())
    }

    fn update_rectangle(&self, reset: bool, host: &mut dyn Host) -> Result<()> {
        let input = self.input(host)?;
        let r = self.config.rectangle;
        let [mut x, mut y] = [r.left, r.top];
        if self.vertical == 0 {
            x = x.wrapping_add(self.control.pixel_position);
        } else {
            y = y.wrapping_add(self.control.pixel_position);
        }
        let rectangle = if reset {
            Rect::default()
        } else {
            Rect::from_xywh(x, y, self.geometry[2], self.geometry[3])
        };
        let mut bytes = [
            rectangle.left,
            rectangle.top,
            rectangle.right,
            rectangle.bottom,
        ]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<_>>();
        self.send(
            host,
            input,
            130,
            [self.buttons[1].unwrap_or(0), 0, 0],
            None,
            Some(&mut bytes),
        )?;
        Ok(())
    }

    fn update_properties(&mut self, update_rectangle: bool, host: &mut dyn Host) -> Result<()> {
        let r = self.config.rectangle;
        let mut position = [
            r.left.wrapping_add(self.config.offset[0]),
            r.top.wrapping_add(self.config.offset[1]),
        ];
        let axis = usize::from(self.vertical != 0);
        position[axis] = position[axis].wrapping_add(self.control.pixel_position);
        self.properties[4..6].copy_from_slice(&position.map(|n| (n as f32).to_bits()));
        self.submit(288, host)?;
        if update_rectangle && !self.control.dragging {
            self.update_rectangle(false, host)?;
        }
        Ok(())
    }

    fn select_state(&mut self, state: i32, host: &mut dyn Host) -> Result<()> {
        if self.frames.is_empty() {
            return Ok(());
        }
        if state == -1 {
            self.send(
                host,
                self.config.list,
                287,
                [self.config.priority, 0, 0],
                None,
                None,
            )?;
        } else {
            let index = usize::try_from(state)
                .ok()
                .filter(|&i| i < 4)
                .ok_or_else(|| Error::new(0, "滑块交互状态超出 0～3"))?;
            self.selected = self.frames.get(&self.states[index]).copied();
            self.properties[0] = match self.selected {
                Some(image) => images(host)?.pixel_handle(image)?,
                None => 0,
            };
            self.submit(288, host)?;
        }
        self.current_state = state;
        Ok(())
    }

    fn notify(&self, code: u32, value: i32, host: &mut dyn Host) -> Result<()> {
        let parent = host.parent_task_id()?;
        self.send(host, parent, code, [0, value as u32, 0], None, None)?;
        Ok(())
    }

    fn button_index(&self, task: u32) -> Option<usize> {
        self.buttons.iter().position(|&button| button == Some(task))
    }

    fn press(&mut self, sender: u32, button: u32, key: u32, host: &mut dyn Host) -> Result<()> {
        let mask = self.send(host, button, 96, [0; 3], None, None)?;
        let index = self.button_index(button);
        if mask & 2 == 0 || index.is_none() {
            self.send(host, sender, 95, [0, 1, 1], None, None)?;
            return Ok(());
        }
        let input = self.input(host)?;
        if key != 0 {
            self.send(host, sender, 95, [0, 1, 1], None, None)?;
            return Ok(());
        }
        if index == Some(0) {
            let point = self.pointer(input, host)?;
            let axis = usize::from(self.vertical != 0);
            let origin = [self.config.rectangle.left, self.config.rectangle.top][axis];
            self.control.pixel_position = point[axis]
                .wrapping_sub(self.geometry[axis + 4] / 2)
                .wrapping_sub(origin)
                .max(self.control.travel[0])
                .min(self.control.travel[1]);
            self.control.position = self.control.position_at_pixel()?;
            self.control.update_pixel_position()?;
            self.update_properties(true, host)?;
            self.update_properties(true, host)?;
            self.notify(27, self.control.position, host)?;
        }
        self.select_state(3, host)?;
        self.control.dragging = true;
        self.update_rectangle(true, host)?;
        let point = self.pointer(input, host)?;
        self.drag_origin = [
            point[0].wrapping_sub(self.config.rectangle.left),
            point[1].wrapping_sub(self.config.rectangle.top),
        ];
        self.drag_pixel = self.control.pixel_position;
        Ok(())
    }

    fn release_press(
        &mut self,
        sender: u32,
        button: u32,
        key: u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        let mask = self.send(host, button, 96, [0; 3], None, None)?;
        let index = self.button_index(button);
        if mask & 2 == 0 || index.is_none() || key != 0 {
            self.send(host, sender, 95, [0, 1, 1], None, None)?;
            return Ok(());
        }
        if index == Some(0) && !self.control.dragging {
            return Ok(());
        }
        self.select_state(0, host)?;
        let input = self.input(host)?;
        self.send(host, input, 146, [0; 3], None, None)?;
        self.control.dragging = false;
        self.update_rectangle(false, host)?;
        self.control.position = self.control.position_at_pixel()?;
        self.control.update_pixel_position()?;
        self.update_properties(true, host)?;
        self.update_properties(true, host)
    }

    fn drag(&mut self, point: [i32; 2], host: &mut dyn Host) -> Result<()> {
        if !self.control.dragging {
            return Ok(());
        }
        let previous = self.control.position;
        if matches!(self.vertical, 0 | 1) {
            let axis = self.vertical as usize;
            let origin = [self.config.rectangle.left, self.config.rectangle.top][axis];
            self.control.pixel_position = self
                .drag_pixel
                .wrapping_sub(self.drag_origin[axis])
                .wrapping_sub(origin)
                .wrapping_add(point[axis]);
        }
        let pixel = self.control.pixel_position;
        self.control.pixel_position = if pixel < self.control.travel[0] {
            self.control.travel[0]
        } else if pixel > self.control.travel[1] {
            self.control.travel[1]
        } else {
            pixel
        };
        self.control.position = self.control.position_at_pixel()?;
        self.update_properties(false, host)?;
        if previous != self.control.position {
            self.notify(27, self.control.position, host)?;
        }
        Ok(())
    }

    fn focus(&mut self, sender: u32, button: u32, focused: u32, host: &mut dyn Host) -> Result<()> {
        if self.button_index(button).is_none() {
            self.send(host, sender, 95, [0, 1, 1], None, None)?;
            return Ok(());
        }
        if self.button_index(button) == Some(1) && !self.control.dragging {
            self.select_state(if focused != 0 { 2 } else { 0 }, host)?;
        }
        let input = self.input(host)?;
        let point = self.pointer(input, host)?;
        let r = self.config.rectangle;
        let inside = i32::from(
            r.left <= point[0] && point[0] < r.right && r.top <= point[1] && point[1] < r.bottom,
        );
        if inside != self.inside {
            self.inside = inside;
            self.notify(28, inside, host)?;
        }
        Ok(())
    }
}

impl TaskObject for SliderObject {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(
        &mut self,
        _host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
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
        if self.base.control().deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let [a, b, _] = command.data;
        match command.code {
            6 => {
                if self.control.dragging {
                    let data = buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "滑块拖动坐标缓冲区缺失"))?;
                    self.drag(
                        [read::u32_at(data, 0)? as i32, read::u32_at(data, 4)? as i32],
                        host,
                    )?;
                }
            }
            9 => self.press(command.sender, a, b, host)?,
            10 => self.release_press(command.sender, a, b, host)?,
            64 => self.focus(command.sender, a, b, host)?,
            65 => {
                let input = self.input(host)?;
                self.send(host, input, 156, [0; 3], None, None)?;
            }
            365 => self.load(
                buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "滑块配置缓冲区缺失"))?,
                host,
            )?,
            366 => {
                if self.loaded {
                    self.send(
                        host,
                        self.config.list,
                        293,
                        [self.config.priority, b, 0],
                        None,
                        None,
                    )?;
                }
            }
            367 => {
                self.select_state(i32::from(b == 0), host)?;
                for task in self.buttons.into_iter().rev() {
                    self.send(host, task.unwrap_or(0), 93, [0, b, 0], None, None)?;
                }
            }
            368 => {
                let data = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "滑块范围缓冲区缺失"))?;
                self.control
                    .set_range([read::u32_at(data, 0)? as i32, read::u32_at(data, 4)? as i32]);
            }
            369 => {
                self.control.set_position(b as i32)?;
                if !self.control.dragging {
                    self.update_properties(true, host)?;
                }
                self.update_properties(!self.control.dragging, host)?;
                if a != 0 {
                    self.notify(27, self.control.position, host)?;
                }
            }
            370 => *output = self.vertical as u32,
            371 => {
                self.alpha = b;
                self.properties[11] = b;
                self.update_properties(false, host)?;
            }
            372 => {
                if let Some(slot) = self.states.get_mut(a as usize) {
                    *slot = b;
                }
            }
            373 => self.select_state(self.current_state, host)?,
            _ => {
                return self.base.dispatch_command(kind, command, output, host, 1);
            }
        }
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
    fn prepare_release(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        self.clear_source(host)?;
        self.loaded = false;
        Ok(true)
    }
}

fn images(host: &mut dyn Host) -> Result<&mut crate::graphics::Images> {
    host.images_mut()
        .ok_or_else(|| Error::new(0, "宿主未提供滑块图像存储"))
}

fn rect(bytes: &[u8]) -> Result<Rect> {
    Ok(Rect {
        left: read::u32_at(bytes, 0)? as i32,
        top: read::u32_at(bytes, 4)? as i32,
        right: read::u32_at(bytes, 8)? as i32,
        bottom: read::u32_at(bytes, 12)? as i32,
    })
}
