use super::{Compositor, Rect, rwipe::RWipe};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 类型 39 的捕获画面遮罩消隐；帧资源随任务释放。
pub struct RWipeObject {
    base: BaseObject,
    id: u32,
    ready: bool,
}

impl RWipeObject {
    pub fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            ready: false,
        }
    }

    fn release(&mut self, host: &mut dyn Host) -> Result<()> {
        let compositor = compositor(host)?;
        compositor.rwipes.remove(&self.id);
        if self.ready {
            compositor.screen_effect = None;
            compositor.auxiliary_targets.clear();
        }
        self.ready = false;
        Ok(())
    }
}

impl TaskObject for RWipeObject {
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
            compositor.rwipes.insert(self.id, RWipe::captured(image));
            compositor.screen_effect = Some(super::compositor::ScreenEffect::RWipe(self.id));
        }
        Ok(())
    }
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if let Some(effect) = compositor(host)?.rwipes.get_mut(&self.id) {
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
                    .rwipes
                    .get(&self.id)
                    .map_or(0, |e| e.state)
            }
            393 if self.ready => {
                let bytes = read::range(buffers.data1.as_deref().unwrap_or_default(), 0, 20)?;
                let values: [u32; 5] = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let mask = if values[3] == 0 {
                    None
                } else {
                    Some(
                        host.images_mut()
                            .ok_or_else(|| Error::new(0, "宿主未提供图像对象服务"))?
                            .pixel_image(values[3])?
                            .surface()
                            .clone(),
                    )
                };
                let [width, height] = host.surface_size().map(|v| v as i32);
                let compositor = compositor(host)?;
                let rect = compositor
                    .drawing_rect
                    .unwrap_or_else(|| Rect::from_xywh(0, 0, width, height));
                let effect = compositor.rwipes.get_mut(&self.id).unwrap();
                effect.far = f32::from_bits(values[1]);
                effect.duration = values[2];
                effect.mode = values[4];
                if effect.state != 0 {
                    effect.restart(now);
                }
                if let Some(mask) = mask {
                    effect.prepare_mask(&mask, rect)?;
                }
                effect.sample(0, false)?;
            }
            394 if self.ready => compositor(host)?
                .rwipes
                .get_mut(&self.id)
                .unwrap()
                .set_state(command.data[1], now),
            393 | 394 => {}
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
