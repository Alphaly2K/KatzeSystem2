use super::{FontRequest, GlyphEffects, GlyphPaint, TextEncoding};
use crate::graphics::{PlaneObject, Rect, Surface};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result};

mod atlas;
mod layout;
use layout::Layout;
pub(crate) use layout::codes as character_set;

/// 类型 8 文本平面，保存字形记录、排版状态和已经绘制的像素。
pub struct TextWindowObject {
    plane: PlaneObject,
    layout: Layout,
    surface: Option<Surface>,
    fonts: Option<[FontRequest; 2]>,
    edge: u32,
    next: usize,
    active: bool,
    breaks: u32,
    atlas: atlas::Atlas,
}

impl TextWindowObject {
    pub fn new(id: u32) -> Self {
        Self {
            plane: PlaneObject::new(id),
            layout: Layout::default(),
            surface: None,
            fonts: None,
            edge: 0xff00_0000,
            next: 0,
            active: false,
            breaks: 0,
            atlas: atlas::Atlas::default(),
        }
    }

    pub fn surface(&self) -> Option<&Surface> {
        self.surface.as_ref()
    }
    pub fn glyph_count(&self) -> usize {
        self.layout.glyphs.len()
    }

    fn plane_command(
        &mut self,
        code: u32,
        data: [u32; 3],
        bytes: Option<&mut [u8]>,
        command: &Command,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        self.plane.dispatch_command_with_buffers(
            kind::TEXT_WINDOW,
            &Command {
                code,
                data,
                ..*command
            },
            &mut CommandBuffers {
                data1: bytes,
                ..Default::default()
            },
            &mut 0,
            host,
            budget,
        )?;
        Ok(())
    }

    fn configure(
        &mut self,
        bytes: &[u8],
        command: &Command,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        if !self.layout.configure(bytes)? {
            return Ok(());
        }
        let c = &self.layout.config;
        let size = [c[2] as u32, c[3] as u32];
        self.surface = Some(Surface::new(size)?);
        if self.fonts.is_none() {
            let defaults = host.message_defaults()?;
            let face = FontRequest {
                encoding: TextEncoding::Cp932,
                name: defaults.font_name.clone(),
                weight: if defaults.bold { 700 } else { 400 },
                italic: false,
                backend: 0,
                glyph_parameter: defaults.glyph_parameter.unwrap_or(1),
                measure_parameter: 0,
            };
            let ruby = FontRequest {
                name: if defaults.ruby_font_name.is_empty() {
                    face.name.clone()
                } else {
                    defaults.ruby_font_name.clone()
                },
                ..face.clone()
            };
            let fonts = host
                .fonts()
                .ok_or_else(|| Error::new(0, "宿主未提供文本窗口字体服务"))?;
            fonts.validate(&face)?;
            fonts.validate(&ruby)?;
            self.fonts = Some([face, ruby]);
        }
        let mut rectangle = [0; 32];
        for (i, v) in [0., 0., 0., size[0] as f32, size[1] as f32, 0., 0., 0.]
            .into_iter()
            .enumerate()
        {
            rectangle[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.plane_command(41, [0; 3], Some(&mut rectangle), command, host, budget)?;
        let mut position = [0; 12];
        for i in 0..2 {
            position[i * 4..i * 4 + 4]
                .copy_from_slice(&(self.layout.config[i] as f32).to_le_bytes());
        }
        self.plane_command(51, [1, 0, 0], Some(&mut position), command, host, budget)?;
        self.publish_whole(host)
    }

    fn paint(&mut self, end: usize, host: &mut dyn Host) -> Result<()> {
        let (Some(surface), Some(requests)) = (&mut self.surface, &self.fonts) else {
            return Ok(());
        };
        let fonts = host
            .fonts()
            .ok_or_else(|| Error::new(0, "宿主未提供文本窗口字体服务"))?;
        for glyph in self.layout.glyphs.iter_mut().take(end) {
            if glyph.drawn || matches!(glyph.code, 0 | 0xffff) {
                continue;
            }
            let size = glyph.size[1].wrapping_sub(2);
            if size <= 0 {
                return Err(Error::new(0, "文本窗口字形尺寸必须为正"));
            }
            let paint = GlyphPaint {
                color: glyph.color,
                shadow_color: self.edge,
                font_size: size as u32,
                double_coverage: false,
                effects: GlyphEffects {
                    shadow: self.layout.config[21] == 1,
                    outline: matches!(self.layout.config[21], 2 | 3),
                    shadow_offset: [2; 2],
                },
            };
            let image = if let Some(image) = self.atlas.render(glyph.code, &paint)? {
                image
            } else {
                let code = glyph.code.to_be_bytes();
                let bytes = if code[0] == 0 { &code[1..] } else { &code[..] };
                let character = if self.atlas.contains(glyph.code) {
                    '\u{3000}'
                } else {
                    crate::text::cp932(bytes)?.chars().next().unwrap()
                };
                let mask =
                    fonts.rasterize(&requests[usize::from(glyph.ruby)], character, size as u32)?;
                let extent = (size as u32)
                    .max(mask.size()[0])
                    .max(mask.size()[1])
                    .checked_add(8)
                    .ok_or_else(|| Error::new(0, "文本窗口字形尺寸溢出"))?;
                let mut image = Surface::new([extent; 2])?;
                mask.paint(&mut image, &paint)?;
                image
            };
            surface.blend_from(&image, image.bounds(), glyph.position, 255);
            glyph.drawn = true;
        }
        Ok(())
    }

    fn publish_whole(&self, host: &mut dyn Host) -> Result<()> {
        if self.layout.config[5] != 0 {
            if let Some(surface) = &self.surface {
                host.submit_message(
                    surface,
                    surface.bounds(),
                    [self.layout.config[0], self.layout.config[1]],
                )?;
            }
        }
        Ok(())
    }

    fn publish(&self, host: &mut dyn Host) -> Result<()> {
        if self.layout.config[17] == 0 {
            return self.publish_whole(host);
        }
        if self.layout.config[5] == 0 {
            return Ok(());
        }
        let Some(surface) = &self.surface else {
            return Ok(());
        };
        let mut bounds: Option<Rect> = None;
        for glyph in self
            .layout
            .glyphs
            .iter()
            .chain(std::iter::once(&layout::Glyph::default()))
        {
            if glyph.drawn {
                let next = Rect::from_xywh(
                    glyph.position[0],
                    glyph.position[1],
                    glyph.size[0],
                    glyph.size[1],
                );
                bounds = Some(bounds.map_or(next, |previous| Rect {
                    left: previous.left.min(next.left),
                    top: previous.top.min(next.top),
                    right: previous.right.max(next.right),
                    bottom: previous.bottom.max(next.bottom),
                }));
            } else if let Some(bounds) = bounds.take() {
                let width = bounds.right.wrapping_sub(bounds.left).wrapping_add(3);
                let height = bounds.bottom.wrapping_sub(bounds.top).wrapping_add(3);
                let rectangle = Rect::from_xywh(
                    if bounds.left > 0 {
                        bounds.left - 1
                    } else {
                        bounds.left
                    },
                    if bounds.top > 0 {
                        bounds.top - 1
                    } else {
                        bounds.top
                    },
                    width.wrapping_add(i32::from(width < self.layout.config[2].wrapping_sub(1))),
                    height.wrapping_add(i32::from(height < self.layout.config[3].wrapping_sub(1))),
                );
                if let Some(rectangle) = rectangle.intersect(surface.bounds()) {
                    host.submit_message(
                        surface,
                        rectangle,
                        [
                            self.layout.config[0].wrapping_add(rectangle.left),
                            self.layout.config[1].wrapping_add(rectangle.top),
                        ],
                    )?;
                }
            }
        }
        Ok(())
    }

    fn start(&mut self, host: &mut dyn Host) -> Result<()> {
        let Some(visible) = host.message_plane_display(self.plane.id())? else {
            return Ok(());
        };
        if visible == 0 {
            self.active = false;
            if let Some(drawings) = host.drawables() {
                drawings.set_display(self.plane.id(), 0);
            }
            return Ok(());
        }
        if matches!(self.layout.config[17], 0 | 1) {
            self.layout
                .schedule(self.next, self.clock().elapsed(), host.frame_rate());
        }
        match self.layout.config[17] {
            0 => {
                if let Some(drawings) = host.drawables() {
                    drawings.set_display(self.plane.id(), 1);
                }
                self.paint(usize::MAX, host)?;
                self.publish(host)?;
            }
            1 => {
                self.paint(self.next, host)?;
                self.publish(host)?;
                self.active = true;
            }
            _ => (),
        }
        Ok(())
    }

    fn reveal(&mut self, now: Option<u32>, host: &mut dyn Host) -> Result<()> {
        while let Some(glyph) = self.layout.glyphs.get(self.next) {
            if glyph.code == 0xffff {
                if self.breaks != u32::MAX && self.breaks != 0 {
                    self.breaks -= 1;
                }
                self.next += 1;
                if self.breaks == 0 {
                    self.active = false;
                    break;
                }
            } else {
                if now.is_some_and(|now| glyph.time > now) {
                    break;
                }
                self.next += 1;
                self.paint(self.next, host)?;
            }
        }
        self.publish(host)?;
        if self.next == self.layout.glyphs.len() {
            self.active = false;
        }
        Ok(())
    }
}

impl TaskObject for TextWindowObject {
    fn control(&self) -> &Control {
        self.plane.control()
    }
    fn clock(&self) -> &TaskClock {
        self.plane.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.plane.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.plane.set_creation_context(context)
    }
    fn register_graphics(&mut self, drawings: &mut crate::graphics::Drawables) -> Result<()> {
        self.plane.register_graphics(drawings)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        if !self.plane.update_body(host, increment, budget)? {
            return Ok(false);
        }
        if self.active && self.layout.config[17] == 1 {
            self.reveal(Some(self.clock().elapsed()), host)?;
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
            102 => {
                if let Some(bytes) = buffers.data1.as_deref() {
                    self.configure(bytes, command, host, budget)?;
                }
            }
            104 => {
                if let Some(bytes) = buffers.data1.as_deref() {
                    if self.layout.append(bytes)? && self.layout.config[17] != 1 {
                        self.paint(usize::MAX, host)?;
                        self.publish(host)?;
                    }
                }
            }
            105 => {
                self.layout.clear();
                if let Some(surface) = &mut self.surface {
                    surface.clear(0);
                }
                self.next = 0;
                self.breaks = 0;
                self.publish_whole(host)?;
            }
            106 => self.layout.config[20] = command.data[1] as i32,
            107 => {
                self.layout.config[17] = command.data[0] as i32;
                self.layout.config[18] = command.data[1] as i32;
                self.next = 0;
            }
            108 => self.layout.config[9] = command.data[1] as i32,
            109 => *output = u32::from(!self.active),
            110 => {
                if host
                    .message_plane_display(self.plane.id())?
                    .is_some_and(|v| v != 0)
                {
                    self.reveal(None, host)?;
                }
            }
            111 => self.layout.config[6] = command.data[1] as i32,
            112 => {
                self.layout.config[19] = command.data[1] as i32;
                self.layout.align();
            }
            113 => {
                if self.layout.glyphs.len() < self.layout.config[15] as u32 as usize {
                    self.layout.glyphs.push(layout::Glyph {
                        code: 0xffff,
                        ..Default::default()
                    });
                }
            }
            114 => {
                if self.breaks != u32::MAX {
                    self.breaks = self.breaks.wrapping_add(command.data[1]);
                }
                self.start(host)?;
            }
            117 | 118 if self.fonts.is_none() => {
                return Err(Error::new(0, "文本窗口用户字形需要先配置字体"));
            }
            117 => self.atlas.set_source(host.pixel_image(command.data[1])?)?,
            118 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "文本窗口用户字形登记需要缓冲区"))?;
                let raw = crate::read::range(bytes, 0, 20)?;
                let area = std::array::from_fn::<_, 4, _>(|i| {
                    crate::read::u32_at(raw, 4 + i * 4).unwrap() as i32
                });
                self.atlas.insert(
                    u16::from_le_bytes(raw[..2].try_into().unwrap()),
                    Rect::from_xywh(area[0], area[1], area[2], area[3]),
                );
            }
            119 => self.layout.cursor = [command.data[0] as i32, command.data[1] as i32],
            121 => self.layout.hanging_enabled = command.data[1] != 0,
            122 => self.edge = command.data[1],
            123 => {
                self.layout.color = command.data[1];
                self.layout.config[16] = command.data[1] as i32;
            }
            124 => {
                let bytes = buffers
                    .data1
                    .as_deref()
                    .ok_or_else(|| Error::new(0, "文本窗口字号表需要缓冲区"))?;
                for i in 0..5 {
                    self.layout.sizes[i] = crate::read::u32_at(bytes, i * 4)? as i32;
                }
            }
            47 => {
                if self.layout.config[5] != 0 {
                    self.plane.dispatch_command_with_buffers(
                        kind, command, buffers, output, host, budget,
                    )?;
                }
                if command.data[1] != 0 {
                    self.start(host)?;
                }
            }
            48 => {
                let effective = if self.plane.creation_flags() != 0 {
                    *command
                } else {
                    Command {
                        data: [command.data[0], 0, command.data[2]],
                        ..*command
                    }
                };
                self.plane.dispatch_command_with_buffers(
                    kind, &effective, buffers, output, host, budget,
                )?;
                if command.data[1] != 0 && self.plane.creation_flags() != 0 {
                    self.start(host)?;
                }
            }
            _ => {
                return self
                    .plane
                    .dispatch_command_with_buffers(kind, command, buffers, output, host, budget);
            }
        }
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.plane.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.plane.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.plane.request_delete(host, budget)
    }
}
