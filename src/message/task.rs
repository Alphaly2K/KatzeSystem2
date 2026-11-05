use super::{
    BreakCharacters, Display, DisplaySettings, GlyphEffects, GlyphMask, LayoutSettings,
    StyleDefaults, TextEncoding, TimingSettings, VerticalAlignment,
};
use crate::graphics::PlaneObject;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{ObjectFactory, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read, text};

/// 字体服务接收原始字体选择参数；度量与绘字必须使用同一份请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontRequest {
    pub encoding: TextEncoding,
    pub name: Vec<u16>,
    pub weight: u32,
    pub backend: u32,
    /// 原字形入口的附加 DWORD，由字体服务处理；不在消息层丢弃。
    pub glyph_parameter: u32,
    /// 原度量入口的开关，与字形入口的附加参数分别保存。
    pub measure_parameter: u32,
}

/// 同步选择字体、度量并生成六十四级灰度遮罩；不支持的请求应返回诊断。
pub trait FontServices {
    /// 注册 CP932 文件名指定的字体；false 表示平台未接受该文件。
    fn add_resource(&mut self, _name: &[u8]) -> Result<bool> {
        Err(Error::new(0, "字体服务未提供字体资源注册"))
    }

    fn validate(&mut self, request: &FontRequest) -> Result<()>;
    fn measure(&mut self, request: &FontRequest, character: char, size: u32) -> Result<i32>;
    fn rasterize(&mut self, request: &FontRequest, character: char, size: u32)
    -> Result<GlyphMask>;
}

struct Configuration {
    bytes: Vec<u8>,
    defaults: StyleDefaults,
    size: [u32; 2],
    settings: DisplaySettings,
    layout: LayoutSettings,
    font: FontRequest,
    offset: [i32; 2],
    submit: bool,
    character_delay: u32,
    period_delay: u32,
    skip_spaces: bool,
}

impl Configuration {
    fn parse(bytes: &[u8], encoding: TextEncoding) -> Result<Option<Self>> {
        let (length, extra) = match encoding {
            TextEncoding::Cp932 => (0xe8, 0),
            TextEncoding::Unicode => (0x12c, 0x40),
        };
        if read::u32_at(bytes, 0)? != length as u32 {
            return Ok(None);
        }
        let bytes = read::range(bytes, 0, length)?;
        let raw = &bytes[4..];
        let word = |offset| read::u32_at(raw, offset);
        if word(0x60)? == 0 {
            return Err(Error::new(0x64, "消息对象的非标记解析模式尚未接入"));
        }
        let defaults = StyleDefaults {
            text_sizes: std::array::from_fn(|i| word(0x18 + i * 4).unwrap()),
            ruby_sizes: std::array::from_fn(|i| word(0x2c + i * 4).unwrap()),
            text_color: word(0xc)?,
            shadow_color: word(0x10)?,
            ruby_color: word(0x14)?,
        };
        let name = raw[0x80..0xc0 + extra]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .take_while(|&c| c != 0)
            .collect::<Vec<_>>();
        if name.len() == (0x40 + extra) / 2 {
            return Err(Error::new(0x84, "消息字体名缺少结束单元"));
        }
        let layout = LayoutSettings {
            encoding,
            width: word(4)? as i32,
            margins: [word(0x40)?, word(0x44)?, word(0x48)?].map(|v| v as i32),
            offset: [0; 2],
            character_spacing: word(0x50)? as i32,
            line_spacing: word(0x54)? as i32,
            normal_size: defaults.text_sizes[2] as i32,
            ruby_offset: word(0x58)? as i32,
            vertical_alignment: match word(0x5c)? {
                1 => VerticalAlignment::Top,
                2 => VerticalAlignment::Bottom,
                _ => VerticalAlignment::Preserve,
            },
            line_breaking: word(0x68)? != 0,
            hanging_indent: word(0x6c)? != 0,
            word_wrap: encoding == TextEncoding::Unicode && word(0x118)? != 0,
            break_characters: BreakCharacters::for_encoding(encoding),
        };
        let tail = 0xdc + extra + usize::from(encoding == TextEncoding::Unicode) * 4;
        Ok(Some(Self {
            bytes: bytes.to_vec(),
            defaults,
            size: [word(4)?, word(8)?],
            settings: DisplaySettings {
                effects: GlyphEffects {
                    shadow: word(0xc4 + extra)? != 0,
                    outline: word(0xc8 + extra)? != 0,
                    ..GlyphEffects::default()
                },
                double_coverage: word(0xc0 + extra)? == 700,
                reveal_width: if word(0)? == 1 {
                    word(0xd0 + extra)?
                } else {
                    0
                },
            },
            layout,
            font: FontRequest {
                encoding,
                name,
                weight: word(0xc0 + extra)?,
                backend: word(0xd4 + extra)?,
                glyph_parameter: word(0xcc + extra)?,
                measure_parameter: word(0x7c)?,
            },
            offset: [read::u32_at(bytes, tail)?, read::u32_at(bytes, tail + 4)?].map(|v| v as i32),
            submit: read::u32_at(bytes, tail + 8)? != 0,
            character_delay: word(0x70)?,
            period_delay: word(0x74)?,
            skip_spaces: word(0x78)? != 0,
        }))
    }

    fn timing(&self, time_scale: Option<u32>) -> TimingSettings {
        TimingSettings {
            encoding: self.font.encoding,
            time_scale,
            character_delay: self.character_delay,
            period_delay: self.period_delay,
            skip_spaces: self.skip_spaces,
        }
    }
}

/// 类型 48／63 的消息平面；共享二维图元属性和任务树生命周期。
pub struct MessageObject {
    plane: PlaneObject,
    encoding: TextEncoding,
    configuration: Option<Configuration>,
    display: Option<Display>,
    active: bool,
    time: u32,
    previous: u32,
    segment: u32,
    complete_snapshot: bool,
}

impl MessageObject {
    pub fn new(id: u32, encoding: TextEncoding) -> Self {
        Self {
            plane: PlaneObject::new(id),
            encoding,
            configuration: None,
            display: None,
            active: false,
            time: 0,
            previous: 0,
            segment: 0,
            complete_snapshot: true,
        }
    }

    pub fn display(&self) -> Option<&Display> {
        self.display.as_ref()
    }
    pub fn configuration_bytes(&self) -> Option<&[u8]> {
        self.configuration.as_ref().map(|c| c.bytes.as_slice())
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn playback_time(&self) -> u32 {
        self.time
    }
    pub fn segment_limit(&self) -> u32 {
        self.segment
    }
    pub fn complete_snapshot(&self) -> bool {
        self.complete_snapshot
    }

    fn prepare(&mut self, host: &mut dyn Host) -> Result<()> {
        let Some(display) = &mut self.display else {
            return Ok(());
        };
        if !display.page().is_dirty() {
            return Ok(());
        }
        let config = self.configuration.as_ref().unwrap();
        let timing = config.timing(host.message_time_scale());
        let fonts = fonts(host)?;
        display.prepare(&config.layout, &timing, |c, s| {
            fonts.measure(&config.font, c, s)
        })?;
        Ok(())
    }

    fn advance(&mut self, now: u32, host: &mut dyn Host) -> Result<bool> {
        self.prepare(host)?;
        let config = self.configuration.as_ref().unwrap();
        let display = self.display.as_mut().unwrap();
        let fonts = fonts(host)?;
        Ok(display
            .advance(now, self.segment, |c, s| {
                fonts.rasterize(&config.font, c, s)
            })?
            .complete)
    }

    fn submit(&mut self, whole: bool, host: &mut dyn Host) -> Result<()> {
        let (Some(display), Some(config)) = (&mut self.display, &self.configuration) else {
            return Ok(());
        };
        if !config.submit {
            return Ok(());
        }
        let rectangle = if whole {
            Some(display.surface().bounds())
        } else {
            display.dirty_rectangle()
        };
        if let Some(rectangle) = rectangle {
            host.submit_message(
                display.surface(),
                rectangle,
                [
                    config.offset[0].wrapping_add(rectangle.left),
                    config.offset[1].wrapping_add(rectangle.top),
                ],
            )?;
            if !whole {
                display.clear_dirty();
                self.complete_snapshot = display.page().remaining(u32::MAX)? == 0;
            }
        }
        Ok(())
    }

    fn start(&mut self, host: &mut dyn Host) -> Result<()> {
        let Some(display) = host.message_plane_display(self.plane.id())? else {
            return Ok(());
        };
        self.active = display != 0;
        host.drawables()
            .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))?
            .set_display(self.plane.id(), u32::from(self.active));
        if self.active {
            self.advance(self.time, host)?;
            let display = self.display.as_mut().unwrap();
            let config = self.configuration.as_ref().unwrap();
            // 显示启动入口始终提交整页，独立于普通脏区提交开关。
            host.submit_message(display.surface(), display.surface().bounds(), config.offset)?;
            display.clear_dirty();
            self.previous = self.clock().elapsed();
        }
        Ok(())
    }

    fn configure(
        &mut self,
        config: Configuration,
        replace: bool,
        command: &Command,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        fonts(host)?.validate(&config.font)?;
        let timing = config.timing(host.message_time_scale());
        let fonts = fonts(host)?;
        if replace {
            self.display.as_mut().unwrap().reconfigure(
                config.defaults,
                config.size,
                config.settings,
                &config.layout,
                &timing,
                |c, s| fonts.measure(&config.font, c, s),
            )?;
        } else {
            let speed = self.display.as_ref().map(|d| d.page().speed());
            let mut display =
                Display::new(config.defaults, self.encoding, config.size, config.settings)?;
            if let Some(speed) = speed {
                display.set_speed(speed);
            }
            self.display = Some(display);
            self.time = 0;
        }
        self.configuration = Some(config);
        if !replace {
            let config = self.configuration.as_ref().unwrap();
            let mut bytes = [0; 32];
            for (i, value) in [
                0,
                0,
                0,
                (config.size[0] as f32).to_bits(),
                (config.size[1] as f32).to_bits(),
                0,
                0,
                0,
            ]
            .into_iter()
            .enumerate()
            {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            self.plane_command(41, [0; 3], Some(&mut bytes), command, host, budget)?;
            let offset = self.configuration.as_ref().unwrap().offset;
            let mut position = [0; 12];
            for (i, value) in [offset[0] as f32, offset[1] as f32, 0.0]
                .into_iter()
                .enumerate()
            {
                position[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            self.plane_command(51, [1, 0, 0], Some(&mut position), command, host, budget)?;
            self.segment = 0;
        }
        self.display.as_mut().unwrap().clear_pixels();
        self.submit(true, host)?;
        if replace {
            self.start(host)?;
        }
        Ok(())
    }

    fn plane_command(
        &mut self,
        code: u32,
        data: [u32; 3],
        buffer: Option<&mut [u8]>,
        command: &Command,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        let kind = match self.encoding {
            TextEncoding::Cp932 => kind::STRING_PLANE,
            TextEncoding::Unicode => kind::STRING_PLANE_UTF8,
        };
        self.plane.dispatch_command_with_buffers(
            kind,
            &Command {
                code,
                data,
                ..*command
            },
            &mut CommandBuffers {
                data0: None,
                data1: buffer,
                external: None,
            },
            &mut 0,
            host,
            budget,
        )?;
        Ok(())
    }
}

impl TaskObject for MessageObject {
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
    fn update_body(
        &mut self,
        host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        // 原文字主体只调用任务基类；时钟、子级和销毁由任务树处理。
        if self.control().deletion_requested == 0 && self.active {
            self.active = !self.advance(self.time, host)?;
            self.submit(false, host)?;
            // 原主体使用传入时刻；previous_time 即使暂停也保存本轮输入。
            let now = self.clock().previous_time;
            self.time = self.time.wrapping_add(now.wrapping_sub(self.previous));
            self.previous = now;
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
        let code = if self.encoding == TextEncoding::Unicode && (476..=496).contains(&command.code)
        {
            command.code - 21
        } else {
            command.code
        };
        let specific = match self.encoding {
            TextEncoding::Cp932 => (455..=475).contains(&command.code),
            TextEncoding::Unicode => (476..=497).contains(&command.code),
        };
        if !specific {
            if command.code == 47 && !self.configuration.as_ref().is_some_and(|c| c.submit) {
                return Ok(CommandOutcome::finished(false));
            }
            let outcome = self
                .plane
                .dispatch_command_with_buffers(kind, command, buffers, output, host, budget)?;
            return if matches!(command.code, 47..=53 | 55 | 56 | 60 | 63 | 66 | 67) {
                Ok(CommandOutcome::finished(false))
            } else {
                Ok(outcome)
            };
        }
        match code {
            455 => {
                if let Some(buffer) = buffers.data1.as_deref_mut() {
                    let length = match self.encoding {
                        TextEncoding::Cp932 => 0xe8,
                        TextEncoding::Unicode => 0x12c,
                    };
                    if read::u32_at(buffer, 0)? == length {
                        let bytes = host.message_defaults()?.configuration(self.encoding)?;
                        buffer
                            .get_mut(..bytes.len())
                            .ok_or_else(|| Error::new(0, "消息默认配置输出缓冲区过短"))?
                            .copy_from_slice(&bytes);
                    }
                }
            }
            456 | 457 => {
                if code == 457 && self.display.is_none() {
                    return Ok(CommandOutcome::finished(false));
                }
                if let Some(bytes) = buffers.data1.as_deref() {
                    if code == 456 {
                        host.drawables()
                            .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))?
                            .remove(self.plane.id());
                        if let Some(display) = &mut self.display {
                            display.clear();
                        }
                    }
                    if let Some(config) = Configuration::parse(bytes, self.encoding)? {
                        self.configure(config, code == 457, command, host, budget)?;
                    }
                }
            }
            458 if self.display.is_some() => {
                let input = read::zstr(
                    buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "消息追加缺少文字缓冲区"))?,
                    0,
                )?;
                let text = match self.encoding {
                    TextEncoding::Cp932 => input
                        .split(|&byte| byte == 0xff)
                        .map(text::cp932)
                        .collect::<Result<Vec<_>>>()?
                        .join("\u{ffff}"),
                    TextEncoding::Unicode => {
                        let text = std::str::from_utf8(input)
                            .map_err(|_| Error::new(0, "消息追加需要有效 UTF-8"))?;
                        if text.encode_utf16().count() >= 4096 {
                            return Err(Error::new(0, "消息 UTF-8 追加超过转换缓冲区范围"));
                        }
                        text.to_owned()
                    }
                };
                self.display.as_mut().unwrap().append(&text)?;
                self.complete_snapshot = false;
            }
            459 => {
                if let Some(display) = &mut self.display {
                    display.clear();
                    self.submit(true, host)?;
                }
                self.time = 0;
                self.active = false;
                self.segment = 0;
            }
            460 | 472 => {
                if self.display.is_none() {
                    return Err(Error::new(0, "消息查询缺少已配置的显示对象"));
                }
                self.prepare(host)?;
                *output = self
                    .display
                    .as_ref()
                    .unwrap()
                    .page()
                    .remaining(if code == 460 { u32::MAX } else { self.segment })?;
            }
            461 if self.display.is_some() => {
                if host.message_plane_display(self.plane.id())?.is_some() {
                    self.advance(u32::MAX, host)?;
                    self.submit(false, host)?;
                    self.active = false;
                }
            }
            462 if self.display.is_some() => self.display.as_mut().unwrap().append("\u{ffff}")?,
            463 if self.display.is_some() => {
                self.segment = command.data[1];
                self.start(host)?;
            }
            467 if self.display.is_some() => {
                self.display.as_mut().unwrap().set_speed(command.data[1])
            }
            466 if self.display.is_some() => {
                let cursor = self
                    .display
                    .as_ref()
                    .unwrap()
                    .page()
                    .layout()
                    .cursor
                    .unwrap_or([0; 2]);
                for (buffer, value) in [&mut buffers.data0, &mut buffers.data1]
                    .into_iter()
                    .zip(cursor)
                {
                    let bytes = buffer
                        .as_deref_mut()
                        .ok_or_else(|| Error::new(0, "消息坐标查询缺少输出缓冲区"))?;
                    bytes
                        .get_mut(..4)
                        .ok_or_else(|| Error::new(0, "消息坐标输出缓冲区过短"))?
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
            469 => {
                *output = 0;
                if self.display.is_some() {
                    self.prepare(host)?;
                    let mut lines = 1u32;
                    let mut newline = false;
                    for entry in &self.display.as_ref().unwrap().page().layout().entries {
                        match entry.kind {
                            super::AtomKind::NewLine => {
                                lines += 1;
                                newline = true;
                            }
                            super::AtomKind::Character(c) if c > '\t' && c != '\u{ffff}' => {
                                newline = false
                            }
                            _ => (),
                        }
                    }
                    *output = lines - u32::from(newline);
                }
            }
            458 | 461 | 462 | 463 | 467 => (),
            466 | 475 => (),
            _ => {
                return Err(Error::new(
                    0,
                    format!("消息对象命令 {} 尚未接入", command.code),
                ));
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

fn fonts(host: &mut dyn Host) -> Result<&mut dyn FontServices> {
    host.fonts()
        .ok_or_else(|| Error::new(0, "宿主未提供消息字体服务"))
}

pub struct MessageFactory<F> {
    pub fallback: F,
}
impl<F: ObjectFactory> ObjectFactory for MessageFactory<F> {
    fn external_storage(&self) -> crate::memory::SharedMemory {
        self.fallback.external_storage()
    }

    fn create_unloaded_ordinary(&mut self, id: u32) -> Result<Box<dyn TaskObject>> {
        self.fallback.create_unloaded_ordinary(id)
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<crate::kcs::Program>> {
        self.fallback.load_kcs_program(name)
    }

    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        match object_kind {
            kind::STRING_PLANE => Ok(Box::new(MessageObject::new(id, TextEncoding::Cp932))),
            kind::STRING_PLANE_UTF8 => Ok(Box::new(MessageObject::new(id, TextEncoding::Unicode))),
            _ => self.fallback.create(id, object_kind, name),
        }
    }
}
