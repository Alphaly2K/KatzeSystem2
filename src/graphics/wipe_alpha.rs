use super::{Rect, Surface};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

#[derive(Clone, Copy, Default)]
struct Parameters {
    duration: u32,
    source: u32,
    source_at: [i32; 2],
    mask: u32,
    at: [i32; 2],
    size: [u32; 2],
    reveal: bool,
    clip_at: [u32; 2],
    clip_size: [u32; 2],
}

impl Parameters {
    fn parse(bytes: Option<&[u8]>) -> Result<Self> {
        let bytes = read::range(bytes.unwrap_or_default(), 0, 48)?;
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        Ok(Self {
            duration: word(0),
            source: word(1),
            source_at: [word(2) as i32, word(3) as i32],
            mask: word(4),
            at: [word(5) as i32, word(6) as i32],
            size: [word(7), word(8)],
            reveal: word(9) != 0,
            clip_at: [word(10), word(11)],
            ..Default::default()
        })
    }
}

/// 类型 28 的 Alpha 遮罩转场；来源和遮罩继续由调用者持有。
pub struct WipeAlphaObject {
    base: BaseObject,
    parameters: Parameters,
    frame: Option<Surface>,
    ready: bool,
    active: bool,
    start: u32,
    speed: f32,
}

impl Default for WipeAlphaObject {
    fn default() -> Self {
        Self::new()
    }
}

impl WipeAlphaObject {
    pub fn new() -> Self {
        Self {
            base: BaseObject::new(),
            parameters: Parameters::default(),
            frame: None,
            ready: false,
            active: false,
            start: 0,
            speed: 1.,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn duration(&self) -> u32 {
        self.parameters.duration
    }

    pub fn size(&self) -> [u32; 2] {
        self.parameters.size
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn start_time(&self) -> u32 {
        self.start
    }

    fn prepare(&mut self, bytes: Option<&[u8]>, host: &mut dyn Host) -> Result<()> {
        self.parameters = Parameters::parse(bytes)?;
        self.frame = None;
        self.ready = false;
        let p = &mut self.parameters;
        if p.source == 0 || p.mask == 0 {
            return Ok(());
        }
        let source = host.image(p.source)?.surface();
        let mask_size = host.image(p.mask)?.info().size;
        p.duration = (p.duration as i32).max(1) as u32;
        for (axis, &mask_size) in mask_size.iter().enumerate() {
            let remaining = p.size[axis]
                .checked_sub(p.clip_at[axis])
                .ok_or_else(|| Error::new(0, "Alpha 转场局部起点超出工作图像"))?;
            p.clip_size[axis] = mask_size.min(remaining);
        }
        let mut frame = Surface::new(p.size)?;
        for y in 0..p.size[1] {
            let source = row(source, p.source_at, y, p.size[0])?;
            let offset = y as usize * frame.stride();
            let output = &mut frame.pixels[offset..offset + source.len()];
            output.copy_from_slice(source);
            if p.reveal {
                for pixel in output.chunks_exact_mut(4) {
                    pixel[3] = 0;
                }
            }
        }
        self.frame = Some(frame);
        self.ready = true;
        Ok(())
    }

    fn submit_full(&self, host: &mut dyn Host) -> Result<()> {
        let p = self.parameters;
        let frame = self
            .frame
            .as_ref()
            .ok_or_else(|| Error::new(0, "Alpha 转场尚未准备工作图像"))?;
        host.submit_message(
            frame,
            frame.bounds(),
            [
                p.at[0].wrapping_add(p.clip_at[0] as i32),
                p.at[1].wrapping_add(p.clip_at[1] as i32),
            ],
        )?;
        Ok(())
    }

    fn finish(&mut self, host: &mut dyn Host) -> Result<()> {
        self.render(self.parameters.duration, host)?;
        self.submit_full(host)?;
        self.active = false;
        Ok(())
    }

    fn render(&mut self, elapsed: u32, host: &mut dyn Host) -> Result<()> {
        let p = self.parameters;
        let numerator = elapsed.min(p.duration).wrapping_shl(9) as i32;
        let progress = (numerator / p.duration as i32).clamp(0, 512);
        let source = host.image(p.source)?.surface();
        let mask = host.image(p.mask)?.surface();
        let frame = self
            .frame
            .as_mut()
            .ok_or_else(|| Error::new(0, "Alpha 转场尚未准备工作图像"))?;
        let source_at = [
            p.source_at[0].wrapping_add(p.clip_at[0] as i32),
            p.source_at[1].wrapping_add(p.clip_at[1] as i32),
        ];
        for y in 0..p.clip_size[1] {
            let source = row(source, source_at, y, p.clip_size[0])?;
            let mask = row(mask, [0; 2], y, p.clip_size[0])?;
            let offset = (p.clip_at[1] + y) as usize * frame.stride() + p.clip_at[0] as usize * 4;
            for ((output, source), mask) in frame.pixels[offset..offset + source.len()]
                .chunks_exact_mut(4)
                .zip(source.chunks_exact(4))
                .zip(mask.chunks_exact(4))
            {
                let weight = (progress + i32::from(mask[0]) - 256).clamp(0, 256) as u32;
                let alpha = ((u32::from(source[3]) * weight) >> 8) as u8;
                output[3] = if p.reveal { alpha } else { !alpha };
            }
        }
        host.submit_message(
            frame,
            Rect::from_xywh(
                p.clip_at[0] as i32,
                p.clip_at[1] as i32,
                p.clip_size[0] as i32,
                p.clip_size[1] as i32,
            ),
            [
                p.at[0].wrapping_add(p.clip_at[0] as i32),
                p.at[1].wrapping_add(p.clip_at[1] as i32),
            ],
        )?;
        Ok(())
    }
}

fn row(image: &Surface, at: [i32; 2], y: u32, width: u32) -> Result<&[u8]> {
    let invalid = || Error::new(0, "Alpha 转场来源区域超出图像范围");
    if at[0] < 0
        || !(at[0] as u32)
            .checked_add(width)
            .is_some_and(|end| end <= image.size()[0])
    {
        return Err(invalid());
    }
    let y = (at[1] as u32)
        .checked_add(y)
        .filter(|_| at[1] >= 0)
        .ok_or_else(invalid)?;
    let y = image.size()[1].checked_sub(y + 1).ok_or_else(invalid)?;
    let start = y as usize * image.stride() + at[0] as usize * 4;
    Ok(&image.pixels()[start..start + width as usize * 4])
}

impl TaskObject for WipeAlphaObject {
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
        if self.base.control().deletion_requested == 0 && self.active {
            let elapsed = ((self.base.clock().previous_time.wrapping_sub(self.start) as f32)
                * self.speed) as u64 as u32;
            if elapsed >= self.parameters.duration {
                self.parameters.clip_at = [0; 2];
                self.submit_full(host)?;
                self.active = false;
            } else {
                self.render(elapsed, host)?;
            }
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
            280 => self.prepare(buffers.data1.as_deref(), host)?,
            263 if self.ready => {
                self.start = self.base.clock().elapsed();
                self.active = true;
                self.speed = match host.effect_speed() {
                    1 => 1.5,
                    2 => 2.,
                    3 => 3.,
                    4 => 4.,
                    _ => 1.,
                };
            }
            264 if self.ready => self.finish(host)?,
            265 => *output = u32::from(self.active),
            266 => {
                let bytes = read::range(buffers.data1.as_deref().unwrap_or_default(), 0, 8)?;
                self.parameters.source = read::u32_at(bytes, 4)?;
            }
            267 if self.active => self.finish(host)?,
            263 | 264 | 267 => (),
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
}
