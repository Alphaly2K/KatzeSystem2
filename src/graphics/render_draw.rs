use super::{DrawCommand, Surface, compositor::ScreenEffect, mosaic::screen_commands};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::sync::Arc;

pub(crate) struct RenderDraw {
    pub(crate) source: Arc<Surface>,
    pub(super) far: f32,
    near: f32,
    duration: u32,
    active: u32,
    now: u32,
    start: u32,
    deadline: u32,
    startup_frames: u8,
    opacity: u8,
    flagged: bool,
}

impl RenderDraw {
    fn new(source: Arc<Surface>) -> Self {
        Self {
            source,
            far: 1_000_000.,
            near: 0.,
            duration: 0,
            active: 0,
            now: 0,
            start: 0,
            deadline: 0,
            startup_frames: 0,
            opacity: 255,
            flagged: false,
        }
    }

    fn restart_clock(&mut self) {
        self.start = self.now;
        self.deadline = self.now.wrapping_add(self.duration);
    }

    fn configure(&mut self, bytes: &[u8]) -> Result<()> {
        let bytes = read::range(bytes, 0, 12)?;
        self.near = f32::from_bits(read::u32_at(bytes, 0)?);
        self.far = f32::from_bits(read::u32_at(bytes, 4)?);
        self.duration = read::u32_at(bytes, 8)?;
        if self.active != 0 {
            self.restart_fade();
        }
        self.duration = self.duration.max(1);
        Ok(())
    }

    fn set_active(&mut self, active: u32) {
        self.active = active;
        if active != 0 {
            self.restart_fade();
        }
    }

    fn restart_fade(&mut self) {
        self.restart_clock();
        self.startup_frames = 3;
        self.opacity = 255;
    }

    pub(super) fn background(&self) -> [DrawCommand; 2] {
        let mut commands = screen_commands(self.source.clone(), 255);
        for command in &mut commands {
            command.texture = None;
            for vertex in &mut command.vertices {
                vertex.color = 0xff00_0000;
            }
        }
        commands
    }

    fn advance(&mut self, now: u32) {
        self.now = now;
        if self.active == 0 {
            return;
        }
        // 前三轮重新写起止时刻；脚本等待不能依赖宿主是否取帧。
        if self.startup_frames != 0 {
            self.startup_frames -= 1;
            self.restart_clock();
        }
        let elapsed = self.now.wrapping_sub(self.start).min(self.deadline);
        self.opacity = 255 - (elapsed.wrapping_mul(255) / self.duration.max(1)).min(255) as u8;
        if self.now >= self.deadline {
            self.active = 0;
        }
    }

    pub(super) fn frame(&self) -> [DrawCommand; 2] {
        let mut commands = screen_commands(self.source.clone(), self.opacity);
        for command in &mut commands {
            for vertex in &mut command.vertices {
                vertex.position[2] = 0.5;
            }
        }
        commands
    }
}

pub(super) struct RenderDrawObject {
    base: BaseObject,
    id: u32,
}

impl RenderDrawObject {
    pub(super) fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
        }
    }

    fn release(&mut self, host: &mut dyn Host) -> Result<()> {
        let compositor = compositor(host)?;
        compositor.render_draws.remove(&self.id);
        if matches!(compositor.screen_effect, Some(ScreenEffect::Draw(id)) if id == self.id) {
            compositor.screen_effect = None;
            compositor.auxiliary_targets.clear();
        }
        Ok(())
    }
}

impl TaskObject for RenderDrawObject {
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
        if host.reserve_auxiliary_targets(1)? {
            let compositor = compositor(host)?;
            if let Some(surface) = compositor.auxiliary_targets[0].tiles()[0].as_ref() {
                compositor
                    .render_draws
                    .insert(self.id, RenderDraw::new(Arc::new(surface.clone())));
                compositor.screen_effect = Some(ScreenEffect::Draw(self.id));
            }
        }
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        let now = self.clock().elapsed();
        if let Some(effect) = compositor(host)?.render_draws.get_mut(&self.id) {
            effect.advance(now);
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
        if self.control().deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        match command.code {
            3 => self.release(host)?,
            383 | 390..=392 => {
                let now = self.clock().elapsed();
                let effect = compositor(host)?.render_draws.get_mut(&self.id);
                if command.code == 383 {
                    *output = effect.map_or(0, |effect| effect.active);
                } else if let Some(effect) = effect {
                    effect.now = now;
                    match command.code {
                        390 => effect.configure(buffers.data1.as_deref().unwrap_or_default())?,
                        391 => effect.set_active(command.data[1]),
                        // 本版的帧回调保留该标记，不用它重置时钟或捕获。
                        392 => effect.flagged = true,
                        _ => unreachable!(),
                    }
                }
                return Ok(CommandOutcome::finished(false));
            }
            _ => {}
        }
        self.base
            .dispatch_command_with_buffers(kind, command, buffers, output, host, budget)
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

fn compositor(host: &mut dyn Host) -> Result<&mut super::Compositor> {
    host.compositor()
        .ok_or_else(|| Error::new(0, "宿主未提供帧合成器"))
}
