use crate::graphics::Rect;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

const WHOLE_SCREEN: Rect = Rect {
    left: -100_000,
    top: -100_000,
    right: 100_000,
    bottom: 100_000,
};

/// 类型 6 的矩形输入任务；独立保存区域、显示门控和同步事件响应。
pub struct ButtonObject {
    id: u32,
    base: BaseObject,
    target: u32,
    parameter: u32,
    response: u32,
    rectangle: Rect,
    display: u32,
    parent_offset: [f32; 3],
    parent_bound: bool,
    full_screen: bool,
    unregistered: bool,
}

impl ButtonObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            target: u32::MAX,
            parameter: 0,
            response: 0,
            rectangle: Rect::default(),
            display: 1,
            parent_offset: [0.; 3],
            parent_bound: false,
            full_screen: false,
            unregistered: false,
        }
    }

    pub fn event_target(&self) -> u32 {
        self.target
    }
    pub fn event_parameter(&self) -> u32 {
        self.parameter
    }
    pub fn response(&self) -> u32 {
        self.response
    }

    pub(crate) fn response_mut(&mut self) -> &mut u32 {
        &mut self.response
    }

    pub(crate) fn take_for_callback(&mut self) -> Self {
        std::mem::replace(self, Self::new(self.id))
    }
    pub fn rectangle(&self) -> Rect {
        self.rectangle
    }
    pub fn display(&self) -> u32 {
        self.display
    }
    pub fn parent_offset(&self) -> Option<[f32; 3]> {
        self.parent_bound.then_some(self.parent_offset)
    }
    pub fn binding_offset(&self) -> [f32; 3] {
        self.parent_offset
    }
    pub fn full_screen(&self) -> bool {
        self.full_screen
    }

    fn send(
        &self,
        host: &mut dyn Host,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<u32> {
        let mut output = 0;
        host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            buffers,
            &mut output,
        )?;
        Ok(output)
    }

    fn input(&self, host: &mut dyn Host) -> Result<u32> {
        self.send(host, 0, 34, [0; 3], &mut CommandBuffers::default())
    }

    fn input_scalar(
        &self,
        host: &mut dyn Host,
        code: u32,
        b: u32,
        propagation: u32,
    ) -> Result<u32> {
        let input = self.input(host)?;
        self.send(
            host,
            input,
            code,
            [self.id, b, propagation],
            &mut CommandBuffers::default(),
        )
    }

    fn notify(
        &mut self,
        host: &mut dyn Host,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<()> {
        let command = Command {
            target: self.target,
            sender: self.id,
            code,
            data,
        };
        if std::env::var_os("KATZE_TRACE_INPUT").is_some() {
            eprintln!(
                "[button-notify] button={} target={} code={} data={:?}",
                self.id, self.target, code, data
            );
        }
        host.dispatch_button_event(self, command, buffers, &mut 0)?;
        Ok(())
    }

    fn is_full_screen(rectangle: Rect, host: &dyn Host) -> bool {
        let [width, height] = host.input_coordinates().logical_size;
        rectangle.left <= 0
            && rectangle.top <= 0
            && rectangle.right >= width
            && rectangle.bottom >= height
    }

    fn unregister(
        &mut self,
        propagation: u32,
        root_propagation: u32,
        host: &mut dyn Host,
    ) -> Result<()> {
        if self.target != u32::MAX {
            let input = self.send(
                host,
                0,
                34,
                [0, 0, root_propagation],
                &mut CommandBuffers::default(),
            )?;
            self.send(
                host,
                input,
                128,
                [0, self.id, propagation],
                &mut CommandBuffers::default(),
            )?;
        }
        Ok(())
    }

    fn set_rectangle(&self, input: u32, rectangle: Rect, host: &mut dyn Host) -> Result<()> {
        let mut bytes = [
            rectangle.left,
            rectangle.top,
            rectangle.right,
            rectangle.bottom,
        ]
        .map(i32::to_le_bytes)
        .concat();
        self.send(
            host,
            input,
            130,
            [self.id, 0, 0],
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
                external: None,
            },
        )?;
        Ok(())
    }

    fn follow_parent(&mut self, host: &mut dyn Host) -> Result<()> {
        if !self.parent_bound {
            return Ok(());
        }
        let offset = self.parent_offset;
        let parent = host.parent_task_id()?;
        let mut position = [0; 12];
        let mut base = [0; 12];
        for (code, bytes) in [(62, &mut position), (61, &mut base)] {
            self.send(
                host,
                parent,
                code,
                [0; 3],
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(bytes),
                    external: None,
                },
            )?;
        }
        let position = words::<3>(Some(&position))?.map(f32::from_bits);
        let base = words::<3>(Some(&base))?.map(f32::from_bits);
        let drawings = host
            .drawables()
            .ok_or_else(|| Error::new(0, "按钮父级绑定需要绘图服务"))?;
        drawings.set_geometry_base_point(
            self.id,
            [-offset[0], -offset[1], offset[2] + base[2]],
            false,
        );
        drawings.set_geometry_position(self.id, position, false);
        let left = truncate((position[0] + base[0]) + offset[0]);
        let top = truncate((position[1] + base[1]) + offset[1]);
        self.rectangle.right = self
            .rectangle
            .right
            .wrapping_add(left.wrapping_sub(self.rectangle.left));
        self.rectangle.bottom = self
            .rectangle
            .bottom
            .wrapping_add(top.wrapping_sub(self.rectangle.top));
        self.rectangle.left = left;
        self.rectangle.top = top;
        let input = self.input(host)?;
        self.set_rectangle(input, self.rectangle, host)
    }
}

impl TaskObject for ButtonObject {
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
    fn begin_update(&mut self) {
        self.response = 0;
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
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let [a, b, propagation] = command.data;
        match command.code {
            3 => {
                self.unregister(propagation, propagation, host)?;
                self.unregistered = true;
                self.base.control.deletion_requested = 1;
            }
            4..=6 => {
                if command.code != 6 {
                    *output = 1;
                }
                let (code, data) = match command.code {
                    4 => (if b != 0 { 9 } else { 10 }, [self.id, a, 0]),
                    5 => (65, [self.id, a, 0]),
                    _ => (6, [a, b, 0]),
                };
                self.notify(host, code, data, buffers)?;
                *output = self.response;
                self.response = 0;
            }
            7 | 8 => self.notify(
                host,
                64,
                [self.id, u32::from(command.code == 7), 0],
                &mut CommandBuffers::default(),
            )?,
            19 => {
                if self.full_screen {
                    let input = self.input(host)?;
                    self.set_rectangle(input, WHOLE_SCREEN, host)?;
                }
            }
            47 => {
                if self.target == u32::MAX {
                    return Ok(CommandOutcome::finished(false));
                }
                let input = self.input(host)?;
                self.display = b;
                let effective = if self.parent_bound {
                    let parent = host.parent_task_id()?;
                    let drawings = host
                        .drawables()
                        .ok_or_else(|| Error::new(0, "按钮显示绑定需要绘图服务"))?;
                    let Some(parent) = drawings.get(parent) else {
                        return Ok(CommandOutcome::finished(false));
                    };
                    parent.display()
                } else {
                    b
                };
                self.send(
                    host,
                    input,
                    135,
                    [
                        self.id,
                        if self.display != 0 { effective } else { 0 },
                        propagation,
                    ],
                    &mut CommandBuffers::default(),
                )?;
                if effective == 0 {
                    self.send(
                        host,
                        input,
                        133,
                        [self.id, 0, propagation],
                        &mut CommandBuffers::default(),
                    )?;
                }
            }
            48 => {
                self.input_scalar(
                    host,
                    135,
                    if self.display != 0 { b } else { 0 },
                    propagation,
                )?;
            }
            50 | 52 => self.follow_parent(host)?,
            53 => {
                self.parent_offset = words::<3>(buffers.data1.as_deref())?.map(f32::from_bits);
                self.parent_bound = true;
                self.follow_parent(host)?;
            }
            54 => self.parent_bound = false,
            90 => {
                *output = 0;
                if self.target != u32::MAX {
                    return Ok(CommandOutcome::finished(false));
                }
                let input = self.input(host)?;
                let values = words::<6>(buffers.data1.as_deref())?;
                let requested = rectangle(values[..4].try_into().unwrap());
                self.full_screen = Self::is_full_screen(requested, host);
                let rect = if self.full_screen {
                    WHOLE_SCREEN
                } else {
                    requested
                };
                let mut region = [0; 60];
                for (index, value) in [
                    self.id,
                    values[4],
                    7,
                    rect.left as u32,
                    rect.top as u32,
                    rect.right as u32,
                    rect.bottom as u32,
                    0,
                ]
                .into_iter()
                .enumerate()
                {
                    region[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                self.send(
                    host,
                    input,
                    127,
                    [values[4], 0, propagation],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut region),
                        external: None,
                    },
                )?;
                self.target = command.sender;
                self.parameter = values[5];
                self.response = 1;
                self.rectangle = rect;
                if std::env::var_os("KATZE_TRACE_NATIVE").is_some() {
                    eprintln!(
                        "[button-input] id={} rect={:?} full={}",
                        self.id, rect, self.full_screen
                    );
                }
                *output = 1;
            }
            91 => {
                if self.target != u32::MAX {
                    self.unregister(propagation, 0, host)?;
                    self.target = u32::MAX;
                    self.response = 1;
                }
            }
            93 | 94 => {
                if self.target != u32::MAX {
                    let input = self.input(host)?;
                    self.send(
                        host,
                        input,
                        if command.code == 93 { 135 } else { 133 },
                        [self.id, b, propagation],
                        &mut CommandBuffers::default(),
                    )?;
                    if command.code == 93 && b == 0 {
                        self.send(
                            host,
                            input,
                            133,
                            [self.id, 0, propagation],
                            &mut CommandBuffers::default(),
                        )?;
                    }
                }
            }
            95 => self.response = b,
            96 => *output = self.input_scalar(host, 137, 0, propagation)?,
            97 => self.target = b,
            98 => {
                self.input_scalar(host, 147, b, propagation)?;
            }
            99 | 100 => {
                if self.target != u32::MAX {
                    self.input_scalar(
                        host,
                        if command.code == 99 { 136 } else { 154 },
                        b,
                        propagation,
                    )?;
                }
            }
            101 => {
                let input = self.input(host)?;
                let requested = rectangle(words::<4>(buffers.data1.as_deref())?);
                self.full_screen = Self::is_full_screen(requested, host);
                self.set_rectangle(
                    input,
                    if self.full_screen {
                        WHOLE_SCREEN
                    } else {
                        requested
                    },
                    host,
                )?;
            }
            _ => {
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
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
    fn request_delete(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        if !self.unregistered {
            self.unregister(0, 0, host)?;
            self.unregistered = true;
        }
        self.base.control.deletion_requested = 1;
        Ok(true)
    }
}

fn words<const N: usize>(buffer: Option<&[u8]>) -> Result<[u32; N]> {
    let buffer = buffer
        .filter(|bytes| bytes.len() >= N * 4)
        .ok_or_else(|| Error::new(0, "按钮输入缓冲区缺失或过短"))?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(buffer[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn rectangle(values: [u32; 4]) -> Rect {
    let [left, top, right, bottom] = values.map(|value| value as i32);
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
