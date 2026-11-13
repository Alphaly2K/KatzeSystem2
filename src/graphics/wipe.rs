use super::{Images, Surface};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

#[derive(Clone, Copy, Default)]
struct Parameters {
    duration: u32,
    source: u32,
    source_at: [i32; 2],
    target: u32,
    target_at: [i32; 2],
    mask: u32,
    at: [i32; 2],
    size: [u32; 2],
    clip_at: [u32; 2],
    clip_size: [u32; 2],
}

impl Parameters {
    fn parse(bytes: Option<&[u8]>) -> Result<Self> {
        let bytes = read::range(bytes.unwrap_or_default(), 0, 56)?;
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        Ok(Self {
            duration: word(0),
            source: word(1),
            source_at: [word(2) as i32, word(3) as i32],
            target: word(4),
            target_at: [word(5) as i32, word(6) as i32],
            mask: word(7),
            at: [word(8) as i32, word(9) as i32],
            size: [word(10), word(11)],
            clip_at: [word(12), word(13)],
            ..Default::default()
        })
    }
}

/// 类型 26 的图像遮罩过渡，来源图像由调用者持有。
pub struct WipeObject {
    base: BaseObject,
    parameters: Parameters,
    frame: Option<Surface>,
    ready: bool,
    active: bool,
    start: u32,
    speed: f32,
}

impl Default for WipeObject {
    fn default() -> Self {
        Self::new()
    }
}

impl WipeObject {
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
        if p.source == 0 || p.target == 0 || p.mask == 0 {
            return Ok(());
        }
        let images = host
            .images_mut()
            .ok_or_else(|| Error::new(0, "宿主未提供遮罩过渡图像存储"))?;
        let from = images.get(p.source)?.info().size;
        let to = images.get(p.target)?.info().size;
        let mask = images.get(p.mask)?.info().size;
        p.duration = (p.duration as i32).max(1) as u32;
        for axis in 0..2 {
            p.size[axis] = from[axis].min(to[axis]);
            for (origin, size) in [
                (p.source_at[axis], from[axis]),
                (p.target_at[axis], to[axis]),
            ] {
                if origin < 0
                    || !(origin as u32)
                        .checked_add(p.size[axis])
                        .is_some_and(|end| end <= size)
                {
                    return Err(Error::new(0, "遮罩过渡来源区域超出图像范围"));
                }
            }
            let remaining = p.size[axis]
                .checked_sub(p.clip_at[axis])
                .ok_or_else(|| Error::new(0, "遮罩过渡局部起点超出图像范围"))?;
            p.clip_size[axis] = mask[axis].min(remaining);
        }
        self.frame = Some(Surface::new(p.size)?);
        for y in 0..p.size[1] {
            for x in 0..p.size[0] {
                let source = pixel(images, p.source, p.source_at, [x, y])?;
                if source >> 24 == 0 {
                    let target = pixel(images, p.target, p.target_at, [x, y])?;
                    set_pixel(images, p.source, p.source_at, [x, y], target & 0x00ff_ffff)?;
                }
            }
        }
        self.ready = true;
        Ok(())
    }

    fn finish(&mut self, host: &mut dyn Host) -> Result<()> {
        let p = self.parameters;
        let source = host.image(p.target)?.surface();
        let frame = self
            .frame
            .as_mut()
            .ok_or_else(|| Error::new(0, "遮罩过渡尚未准备工作图像"))?;
        for y in 0..p.size[1] {
            let source = row(source, p.target_at, y, p.size[0])?;
            let offset = y as usize * frame.stride();
            frame.pixels[offset..offset + source.len()].copy_from_slice(source);
        }
        host.submit_message(frame, frame.bounds(), p.at)?;
        self.active = false;
        Ok(())
    }

    fn render(&mut self, elapsed: u32, host: &mut dyn Host) -> Result<()> {
        let p = self.parameters;
        let numerator = elapsed.min(p.duration).wrapping_shl(9) as i32;
        let progress = (numerator / p.duration as i32).clamp(0, 512);
        let source = host.image(p.source)?.surface();
        let target = host.image(p.target)?.surface();
        let mask = host.image(p.mask)?.surface();
        let frame = self
            .frame
            .as_mut()
            .ok_or_else(|| Error::new(0, "遮罩过渡尚未准备工作图像"))?;
        let origin = |at: [i32; 2]| {
            [
                at[0].wrapping_add(p.clip_at[0] as i32),
                at[1].wrapping_add(p.clip_at[1] as i32),
            ]
        };
        for y in 0..p.clip_size[1] {
            let from = row(source, origin(p.source_at), y, p.clip_size[0])?;
            let to = row(target, origin(p.target_at), y, p.clip_size[0])?;
            let shades = row(mask, [0; 2], y, p.clip_size[0])?;
            let offset = (p.clip_at[1] + y) as usize * frame.stride() + p.clip_at[0] as usize * 4;
            for (((out, from), to), shade) in frame.pixels[offset..offset + from.len()]
                .chunks_exact_mut(4)
                .zip(from.chunks_exact(4))
                .zip(to.chunks_exact(4))
                .zip(shades.chunks_exact(4))
            {
                let weight = (progress + i32::from(shade[0]) - 256).clamp(0, 256) as u32;
                for channel in 0..4 {
                    out[channel] = ((u32::from(from[channel]) * (256 - weight)
                        + u32::from(to[channel]) * weight)
                        >> 8) as u8;
                }
            }
        }
        host.submit_message(frame, frame.bounds(), p.at)?;
        Ok(())
    }
}

fn row(image: &Surface, at: [i32; 2], y: u32, width: u32) -> Result<&[u8]> {
    let invalid = || Error::new(0, "遮罩过渡来源图像尺寸已改变");
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

fn pixel(images: &Images, handle: u32, at: [i32; 2], point: [u32; 2]) -> Result<u32> {
    let image = images.get(handle)?.surface();
    let bytes = row(image, at, point[1], point[0] + 1)?;
    Ok(u32::from_le_bytes(
        bytes[point[0] as usize * 4..point[0] as usize * 4 + 4]
            .try_into()
            .unwrap(),
    ))
}

fn set_pixel(
    images: &mut Images,
    handle: u32,
    at: [i32; 2],
    point: [u32; 2],
    value: u32,
) -> Result<()> {
    let image = images.get_mut(handle)?;
    let x = at[0] as u32 + point[0];
    let y = image.info().size[1] - at[1] as u32 - point[1] - 1;
    let offset = (y as usize * image.info().size[0] as usize + x as usize) * 4;
    image.pixels_mut()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

impl TaskObject for WipeObject {
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
                self.finish(host)?;
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
            280 => {
                self.prepare(buffers.data1.as_deref(), host)?;
            }
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
                let bytes = read::range(buffers.data1.as_deref().unwrap_or_default(), 0, 20)?;
                self.parameters.source = read::u32_at(bytes, 4)?;
                self.parameters.target = read::u32_at(bytes, 16)?;
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
