use super::{
    Compositor,
    compositor::ScreenEffect,
    screen_filter::{FilterKind, ScreenFilter},
};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

/// 类型 35～37 的实时残影、马赛克与阴影。
pub struct ScreenFilterObject {
    base: BaseObject,
    id: u32,
    filter: FilterKind,
    ready: bool,
}

impl ScreenFilterObject {
    pub fn blur(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            filter: FilterKind::Blur,
            ready: false,
        }
    }

    pub fn mosaic(id: u32) -> Self {
        Self {
            filter: FilterKind::Mosaic,
            ..Self::blur(id)
        }
    }

    pub fn shade(id: u32) -> Self {
        Self {
            filter: FilterKind::Shade,
            ..Self::blur(id)
        }
    }

    fn release(&mut self, host: &mut dyn Host) -> Result<()> {
        if self.ready {
            let compositor = compositor(host)?;
            compositor.screen_filters.remove(&self.id);
            compositor.screen_effect = None;
            compositor.auxiliary_targets.clear();
            self.ready = false;
        }
        Ok(())
    }
}

impl TaskObject for ScreenFilterObject {
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

    fn initialize_resources(&mut self, host: &mut dyn Host) -> Result<()> {
        self.ready = host.reserve_auxiliary_targets(self.filter.target_count() as u32)?;
        if self.ready {
            let size = host.surface_size();
            let logical = host.input_coordinates().logical_size;
            let scale = std::array::from_fn(|i| {
                if logical[i] > 0 {
                    size[i] as f32 / logical[i] as f32
                } else {
                    1.
                }
            });
            let compositor = compositor(host)?;
            compositor
                .screen_filters
                .insert(self.id, ScreenFilter::new(size, scale, self.filter));
            compositor.screen_effect = Some(ScreenEffect::Filter(self.id));
        }
        Ok(())
    }

    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
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
            383 => {
                *output = compositor(host)?
                    .screen_filters
                    .get(&self.id)
                    .map_or(0, |effect| effect.active);
            }
            code if self.filter.accepts(code) => {
                if self.ready {
                    let effect = compositor(host)?.screen_filters.get_mut(&self.id).unwrap();
                    if code % 2 == 0 {
                        effect.configure(buffers.data1.as_deref().unwrap_or_default())?;
                    } else {
                        effect.set_active(command.data[1]);
                    }
                }
            }
            3 => {
                self.release(host)?;
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
            }
            _ => {
                return self
                    .base
                    .dispatch_command_with_buffers(kind, command, buffers, output, host, budget);
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
        self.release(host)?;
        Ok(true)
    }
}

fn compositor(host: &mut dyn Host) -> Result<&mut Compositor> {
    host.compositor()
        .ok_or_else(|| Error::new(0, "宿主未提供帧合成器"))
}
