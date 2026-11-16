use super::{Drawables, vertex_wave::VertexWave};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 类型 18 的顶点波形任务；参数指定被变形的绘图对象。
pub struct WaveObject {
    id: u32,
    base: BaseObject,
    target: u32,
    wave: VertexWave,
    active: bool,
    previous: u32,
}

impl WaveObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            target: 0,
            wave: VertexWave::default(),
            active: false,
            previous: 0,
        }
    }

    fn publish(&self, host: &mut dyn Host) -> Result<()> {
        for (owner, wave) in drawings(host)?.vertex_waves.values_mut() {
            if *owner == self.id {
                *wave = self.wave;
            }
        }
        Ok(())
    }
}

fn drawings(host: &mut dyn Host) -> Result<&mut Drawables> {
    host.drawables()
        .ok_or_else(|| Error::new(0, "宿主未提供波形绘图存储"))
}

impl TaskObject for WaveObject {
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
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.active {
            let now = self.base.clock().elapsed();
            self.wave
                .advance(now.wrapping_sub(self.previous), host.effect_speed());
            self.previous = now;
            self.publish(host)?;
        }
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
        if self.base.control().deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        match command.code {
            269 | 270 => {
                let bytes = read::range(buffers.data1.as_deref().unwrap_or_default(), 0, 20)?;
                if command.code == 269 {
                    self.target = self.wave.configure(bytes)?;
                } else {
                    self.wave.change(bytes)?;
                }
                self.publish(host)?;
            }
            263 => {
                self.active = true;
                self.wave.phase = 0;
                self.previous = self.base.clock().elapsed();
                self.publish(host)?;
                let drawings = drawings(host)?;
                if drawings.get(self.target).is_some() {
                    drawings
                        .vertex_waves
                        .insert(self.target, (self.id, self.wave));
                }
            }
            264 => {
                self.active = false;
                drawings(host)?.vertex_waves.remove(&self.target);
            }
            265 => *output = u32::from(self.active),
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
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
    fn prepare_release(&mut self, host: &mut dyn Host, _: usize) -> Result<bool> {
        if self.active {
            drawings(host)?.vertex_waves.remove(&self.target);
        }
        drawings(host)?
            .vertex_waves
            .retain(|_, (owner, _)| *owner != self.id);
        Ok(true)
    }
}
