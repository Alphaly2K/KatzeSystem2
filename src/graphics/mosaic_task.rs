use super::{
    Compositor,
    mosaic::{Mosaic, MosaicKind},
};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 类型 50 的双画面马赛克与类型 51 的马赛克淡出；帧资源随任务释放。
pub struct MosaicObject {
    base: BaseObject,
    id: u32,
    ready: bool,
    kind: MosaicKind,
}

impl MosaicObject {
    pub fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            ready: true,
            kind: MosaicKind::Dual,
        }
    }

    pub fn fading(id: u32) -> Self {
        Self {
            kind: MosaicKind::Fade,
            ..Self::new(id)
        }
    }

    fn capture(&mut self, index: usize, host: &mut dyn Host) -> Result<()> {
        let available = host.reserve_auxiliary_targets(2)?;
        let compositor = compositor(host)?;
        let image = available
            .then_some(&compositor.auxiliary_targets)
            .and_then(|targets| targets.get(index))
            .and_then(|target| target.tiles()[0].as_ref())
            .cloned();
        self.ready = image.is_some();
        if let Some(image) = image {
            compositor
                .mosaics
                .entries
                .entry(self.id)
                .or_default()
                .capture(index, image)?;
            compositor.mosaics.selected = Some(self.id);
        } else {
            compositor.auxiliary_targets.clear();
        }
        Ok(())
    }

    fn release(&mut self, host: &mut dyn Host) -> Result<()> {
        let compositor = compositor(host)?;
        compositor.mosaics.remove(self.id);
        if self.ready {
            compositor.auxiliary_targets.clear();
        }
        self.ready = false;
        Ok(())
    }
}

impl TaskObject for MosaicObject {
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
        if self.kind != MosaicKind::Fade {
            return Ok(());
        }
        let available = host.reserve_auxiliary_targets(1)?;
        let compositor = compositor(host)?;
        let image = available
            .then_some(&compositor.auxiliary_targets)
            .and_then(|targets| targets.first())
            .and_then(|target| target.tiles()[0].as_ref())
            .cloned();
        compositor.auxiliary_targets.clear();
        self.ready = image.is_some();
        if let Some(image) = image {
            compositor
                .mosaics
                .entries
                .insert(self.id, Mosaic::fading(image));
            compositor.mosaics.selected = Some(self.id);
        }
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if let Some(effect) = compositor(host)?.mosaics.entries.get_mut(&self.id) {
            effect.now = self.base.clock().elapsed();
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
                    .mosaics
                    .entries
                    .get(&self.id)
                    .map_or(0, |effect| effect.state);
            }
            525 | 528 if self.ready && (command.code == 528) == (self.kind == MosaicKind::Fade) => {
                let bytes = read::range(
                    buffers.data1.as_deref().unwrap_or_default(),
                    0,
                    if self.kind == MosaicKind::Fade {
                        24
                    } else {
                        20
                    },
                )?;
                let values: [u32; 5] = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let effect = compositor(host)?
                    .mosaics
                    .entries
                    .entry(self.id)
                    .or_default();
                effect.far = f32::from_bits(values[1]);
                effect.duration = values[2];
                effect.block = values[3] as i32;
                effect.algorithm = values[4];
                if self.kind == MosaicKind::Dual && effect.duration < 2 {
                    self.ready = false;
                    return Ok(CommandOutcome::finished(false));
                }
                if effect.state != 0 {
                    effect.restart(now);
                }
                if self.kind == MosaicKind::Fade {
                    effect.prepare_fade()?;
                } else {
                    self.capture(0, host)?;
                }
            }
            526 if self.ready && self.kind == MosaicKind::Dual => {
                self.capture(1, host)?;
                compositor(host)?.auxiliary_targets.clear();
            }
            527 | 529 if self.ready && (command.code == 529) == (self.kind == MosaicKind::Fade) => {
                compositor(host)?
                    .mosaics
                    .entries
                    .entry(self.id)
                    .or_insert_with(Mosaic::default)
                    .set_state(command.data[1], now);
            }
            525..=529 if (command.code >= 528) == (self.kind == MosaicKind::Fade) => {}
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
