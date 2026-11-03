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
    at: [i32; 2],
    size: [i32; 2],
}

impl Parameters {
    fn parse(bytes: Option<&[u8]>) -> Result<Self> {
        let bytes = read::range(bytes.unwrap_or_default(), 0, 44)?;
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        Ok(Self {
            duration: word(0),
            source: word(1),
            source_at: [word(2) as i32, word(3) as i32],
            target: word(4),
            target_at: [word(5) as i32, word(6) as i32],
            at: [word(7) as i32, word(8) as i32],
            size: [word(9) as i32, word(10) as i32],
        })
    }
}

/// 类型 27 的画布淡入淡出任务；来源图像由创建者持有。
pub struct FadeObject {
    base: BaseObject,
    parameters: Parameters,
    frame: Option<Surface>,
    ready: bool,
    active: bool,
    start: u32,
    speed: f32,
}

impl Default for FadeObject {
    fn default() -> Self {
        Self::new()
    }
}

impl FadeObject {
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

    pub fn size(&self) -> [i32; 2] {
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
        if p.source == 0 || p.target == 0 {
            return Ok(());
        }
        let images = host
            .images_mut()
            .ok_or_else(|| Error::new(0, "宿主未提供淡入淡出图像存储"))?;
        let from = images.get(p.source)?.info().size;
        let to = images.get(p.target)?.info().size;
        p.duration = (p.duration as i32).max(1) as u32;
        for axis in 0..2 {
            let a = p.source_at[axis];
            let b = p.target_at[axis];
            if a < 0 || b < 0 || a as u32 > from[axis] || b as u32 > to[axis] {
                return Err(Error::new(0, "淡入淡出来源起点超出图像范围"));
            }
            p.size[axis] = p.size[axis]
                .min(from[axis] as i32 - a)
                .min(to[axis] as i32 - b);
            if p.size[axis] < 0 {
                return Err(Error::new(0, "淡入淡出尺寸不能为负数"));
            }
        }
        self.frame = Some(Surface::new(p.size.map(|n| n as u32))?);
        for y in 0..p.size[1] {
            for x in 0..p.size[0] {
                let from = pixel(images, p.source, p.source_at, [x, y])?;
                let to = pixel(images, p.target, p.target_at, [x, y])?;
                if from >> 24 == 0 {
                    set_pixel(images, p.source, p.source_at, [x, y], to & 0x00ff_ffff)?;
                } else if to >> 24 == 0 {
                    set_pixel(images, p.target, p.target_at, [x, y], from & 0x00ff_ffff)?;
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
            .ok_or_else(|| Error::new(0, "淡入淡出尚未准备工作图像"))?;
        for y in 0..p.size[1] {
            let from = row(source, p.target_at, y, p.size[0] as u32)?;
            let offset = y as usize * frame.stride();
            frame.pixels[offset..offset + from.len()].copy_from_slice(from);
        }
        host.submit_message(frame, frame.bounds(), p.at)?;
        self.active = false;
        Ok(())
    }

    fn render(&mut self, elapsed: u32, host: &mut dyn Host) -> Result<()> {
        let p = self.parameters;
        let numerator = elapsed.min(p.duration).wrapping_shl(8) as i32;
        let weight = (numerator / p.duration as i32).clamp(0, 256) as u32;
        let images = host
            .images_mut()
            .ok_or_else(|| Error::new(0, "宿主未提供淡入淡出图像存储"))?;
        let source = images.get(p.source)?.surface();
        let target = images.get(p.target)?.surface();
        let frame = self
            .frame
            .as_mut()
            .ok_or_else(|| Error::new(0, "淡入淡出尚未准备工作图像"))?;
        for y in 0..p.size[1] {
            let from = row(source, p.source_at, y, p.size[0] as u32)?;
            let to = row(target, p.target_at, y, p.size[0] as u32)?;
            // 画布复制依次消费原图像的物理行；提交面已按顶部起算，不再反转。
            let offset = y as usize * frame.stride();
            for ((out, &from), &to) in frame.pixels[offset..offset + from.len()]
                .iter_mut()
                .zip(from)
                .zip(to)
            {
                *out = ((u32::from(from) * (256 - weight) + u32::from(to) * weight) >> 8) as u8;
            }
        }
        host.submit_message(frame, frame.bounds(), p.at)?;
        Ok(())
    }
}

fn row(image: &Surface, at: [i32; 2], y: i32, width: u32) -> Result<&[u8]> {
    let invalid = || Error::new(0, "淡入淡出来源图像尺寸已改变");
    if at[0] < 0
        || !(at[0] as u32)
            .checked_add(width)
            .is_some_and(|end| end <= image.size()[0])
    {
        return Err(invalid());
    }
    let y = at[1]
        .checked_add(y)
        .filter(|&n| n >= 0)
        .ok_or_else(invalid)?;
    let y = image.size()[1]
        .checked_sub(y as u32 + 1)
        .ok_or_else(invalid)?;
    let start = y as usize * image.stride() + at[0] as usize * 4;
    Ok(&image.pixels()[start..start + width as usize * 4])
}

fn pixel(images: &Images, handle: u32, at: [i32; 2], point: [i32; 2]) -> Result<u32> {
    let image = images.get(handle)?.surface();
    let invalid = || Error::new(0, "淡入淡出来源图像尺寸已改变");
    let x = at[0]
        .checked_add(point[0])
        .filter(|&n| n >= 0)
        .ok_or_else(invalid)?;
    let y = at[1]
        .checked_add(point[1])
        .filter(|&n| n >= 0)
        .ok_or_else(invalid)?;
    let y = image.size()[1]
        .checked_sub(y as u32 + 1)
        .ok_or_else(invalid)?;
    image.pixel(x as u32, y).ok_or_else(invalid)
}

fn set_pixel(
    images: &mut Images,
    handle: u32,
    at: [i32; 2],
    point: [i32; 2],
    value: u32,
) -> Result<()> {
    let image = images.get_mut(handle)?;
    let x = at[0] + point[0];
    let y = image.info().size[1] as i32 - at[1] - point[1] - 1;
    let offset = (y as usize * image.info().size[0] as usize + x as usize) * 4;
    image.pixels_mut()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

impl TaskObject for FadeObject {
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
            281 => {
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
                let a = host.image(self.parameters.source)?.info().size;
                let b = host.image(self.parameters.target)?.info().size;
                if a != b {
                    return Err(Error::new(0, "淡入淡出替换来源的图像尺寸不相等"));
                }
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
