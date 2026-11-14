use std::sync::Arc;

use super::{Compositor, page_flip::PageFlip};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 类型 57 的捕获画面卷曲翻转。
pub struct PageFlipObject {
    base: BaseObject,
    id: u32,
    ready: bool,
}

impl PageFlipObject {
    pub fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            ready: false,
        }
    }

    fn release(&mut self, host: &mut dyn Host) -> Result<()> {
        let compositor = compositor(host)?;
        compositor.page_flips.remove(&self.id);
        if self.ready {
            compositor.screen_effect = None;
            compositor.auxiliary_targets.clear();
        }
        self.ready = false;
        Ok(())
    }
}

impl TaskObject for PageFlipObject {
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
        self.ready = host.reserve_auxiliary_targets(1)?;
        if self.ready {
            let compositor = compositor(host)?;
            let image = compositor
                .auxiliary_targets
                .first()
                .and_then(|target| target.tiles()[0].as_ref())
                .cloned()
                .map(Arc::new);
            self.ready = image.is_some();
            if let Some(image) = image {
                compositor
                    .page_flips
                    .insert(self.id, PageFlip::captured(image)?);
                compositor.screen_effect = Some(super::compositor::ScreenEffect::PageFlip(self.id));
            }
        }
        Ok(())
    }
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if let Some(effect) = compositor(host)?.page_flips.get_mut(&self.id) {
            effect.update(self.base.clock().elapsed());
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
        let now = self.base.clock().elapsed();
        match command.code {
            383 => {
                *output = compositor(host)?
                    .page_flips
                    .get(&self.id)
                    .map_or(0, |effect| effect.state);
            }
            396 if self.ready => {
                let bytes = read::range(buffers.data1.as_deref().unwrap_or_default(), 0, 24)?;
                let values = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let width = host.surface_size()[0];
                compositor(host)?
                    .page_flips
                    .get_mut(&self.id)
                    .unwrap()
                    .configure(values, now, width);
            }
            397 if self.ready => {
                compositor(host)?
                    .page_flips
                    .get_mut(&self.id)
                    .unwrap()
                    .set_state(command.data[1], now);
            }
            // 此版本的绘制回调不消费命令 398 设置的标志。
            396..=398 => {}
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
