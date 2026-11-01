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
    update_property: u32,
    blend_mode: u32,
    creation_flags: u32,
    offset: [f32; 2],
    size: [f32; 2],
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
            update_property: 1,
            blend_mode: 0,
            creation_flags: 1,
            offset: [0.0; 2],
            size: [0.0; 2],
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

    pub fn shape_kind(&self) -> u32 {
        self.shape_kind
    }

    /// 上次成功装载的 KX2 名称；清除图元或装载失败不改写此值。
    pub fn geometry_name(&self) -> &[u8] {
        &self.geometry_name
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
                    self.control.deletion_requested = 1;
                }
            }
            42 | 45 => {
                drawings(host)?.remove(id);
                let drawable = if command.code == 42 {
                    let words = words::<8>(buffers.data1.as_deref())?;
                    Drawable::rectangle(
                        [
                            f32::from_bits(words[0]),
                            f32::from_bits(words[1]),
                            f32::from_bits(words[2]),
                        ],
                        [f32::from_bits(words[3]), f32::from_bits(words[4])],
                        words[7],
                    )
                } else {
                    Drawable::default()
                };
                if self.creation_flags == 1 {
                    let bounds = drawable.bounds();
                    self.offset = [0.0; 2];
                    self.size =
                        std::array::from_fn(|i| arithmetic(2, bounds[1][i], bounds[0][i]).abs());
                    self.shape_kind = if command.code == 42 { 2 } else { 5 };
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
            49 | 51 | 55 | 66 => {
                let value = words::<3>(buffers.data1.as_deref())?.map(f32::from_bits);
                let drawings = drawings(host)?;
                let next = match command.code {
                    49 => {
                        drawings.set_base_point(id, value);
                        50
                    }
                    51 => {
                        drawings.set_position(id, value);
                        52
                    }
                    55 => {
                        drawings.set_rotation(id, value);
                        56
                    }
                    _ => {
                        drawings.set_scale(id, value);
                        67
                    }
                };
                self.forward(next, command, buffers, output, host)?;
            }
            50 | 52 | 56 | 58 | 67 => {
                self.forward(command.code, command, buffers, output, host)?;
            }
            61 | 62 => {
                if let Some(drawable) = drawings(host)?.get(id) {
                    let value = if command.code == 61 {
                        drawable.transform().base_point()
                    } else {
                        drawable.transform().position()
                    };
                    write_words(buffers.data1.as_deref_mut(), &value.map(f32::to_bits))?;
                }
            }
            59 => {
                drawings(host)?.set_color(id, command.data[1], command.data[0]);
            }
            63 => {
                drawings(host)?.set_alpha(id, command.data[1]);
            }
            74 => *output = self.input_state,
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
            78 => self.update_property = command.data[1],
            81 => {
                let value = command.data[1];
                self.blend_mode = if value <= 5 { value } else { 0 };
                drawings(host)?.set_blend_mode(id, value);
            }
            84 => self.creation_flags = command.data[1],
            57 => {
                let value = words::<16>(buffers.data1.as_deref())?.map(f32::from_bits);
                drawings(host)?.set_matrix(id, value);
                self.forward(58, command, buffers, output, host)?;
            }
            60 | 71 | 85 | 86 => {}
            4 | 7 | 8 | 41..=99 | 591 | 592 => {
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
        self.control.deletion_requested = 1;
        Ok(true)
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

/// 创建二维平面，其余类型委托已有工厂。
pub struct GraphicsFactory<F> {
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for GraphicsFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::PLANE {
            Ok(Box::new(PlaneObject::new(id)))
        } else {
            self.fallback.create(id, object_kind, name)
        }
    }
}
