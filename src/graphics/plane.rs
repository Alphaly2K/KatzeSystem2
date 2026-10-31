use super::Drawables;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 二维平面任务的基础生命周期和不携带指针的绘图属性命令。
pub struct PlaneObject {
    id: u32,
    control: Control,
    clock: TaskClock,
    input_state: u32,
    update_property: u32,
    blend_mode: u32,
    creation_flags: u32,
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
        _budget: usize,
    ) -> Result<CommandOutcome> {
        if self.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        let id = self.id;
        match command.code {
            3 | 68 => {
                drawings(host)?.remove(id);
                if command.code == 3 {
                    self.control.deletion_requested = 1;
                }
            }
            59 => {
                drawings(host)?.set_color(id, command.data[1], command.data[0]);
            }
            63 => {
                drawings(host)?.set_alpha(id, command.data[1]);
            }
            74 => *output = self.input_state,
            78 => self.update_property = command.data[1],
            81 => {
                let value = command.data[1];
                self.blend_mode = if value <= 5 { value } else { 0 };
                drawings(host)?.set_blend_mode(id, value);
            }
            84 => self.creation_flags = command.data[1],
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
