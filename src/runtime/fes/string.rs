use super::{ObjectState, Objects};
use crate::runtime::{ObjectFactory, Store, kind, route_with_buffers};
use crate::task::{Command, CommandBuffers};
use crate::{
    Error, Result, expression, fes,
    message::{MessageDefaults, TextEncoding},
};

#[derive(Clone, Debug, Default)]
pub(super) struct Context {
    unicode: bool,
    defaults: MessageDefaults,
    measurement: u32,
}

/// FES 普通文字对象的配置、累计文本、待下发请求及上次更新的查询结果。
#[derive(Clone, Debug, PartialEq)]
pub struct StringState {
    pub layout: fes::StringState,
    kind: &'static str,
    initial: bool,
    text: Option<Vec<u8>>,
    consumed: usize,
    append_pending: bool,
    lines_dirty: bool,
    replace: bool,
    clear: bool,
    draw: Option<u32>,
    skip: bool,
    remaining: [i32; 2],
    lines: u32,
    cursor: [i32; 2],
}

impl StringState {
    pub(super) fn new(kind: &str, context: &Context) -> Result<Self> {
        Ok(Self {
            layout: fes::StringState::new(
                if kind == "STRING8" || context.unicode {
                    TextEncoding::Unicode
                } else {
                    TextEncoding::Cp932
                },
                &context.defaults,
                context.measurement,
            )?,
            kind: if kind == "STRING8" {
                "STRING8"
            } else {
                "STRING"
            },
            initial: true,
            text: None,
            consumed: 0,
            append_pending: false,
            lines_dirty: false,
            replace: false,
            clear: false,
            draw: None,
            skip: false,
            remaining: [0; 2],
            lines: 0,
            cursor: [0; 2],
        })
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }
    pub fn text(&self) -> Option<&[u8]> {
        self.text.as_deref()
    }
    pub fn remaining(&self) -> i32 {
        self.remaining[0]
    }
    pub fn remaining_segment(&self) -> i32 {
        self.remaining[1]
    }
    pub fn line_count(&self) -> u32 {
        self.lines
    }
    pub fn cursor(&self) -> [i32; 2] {
        self.cursor
    }

    pub(super) fn task_kind(&self) -> u32 {
        if self.layout.encoding() == TextEncoding::Unicode {
            kind::STRING_PLANE_UTF8
        } else {
            kind::STRING_PLANE
        }
    }

    fn set_text(&mut self, text: Vec<u8>) {
        self.text = Some(text);
        self.consumed = 0;
        self.append_pending = true;
        self.replace = true;
        self.clear = true;
        self.remaining[0] = -1;
    }

    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        let current = self.text.as_ref().map_or(0, Vec::len);
        if bytes.len() > crate::read::MAX_BYTES - current {
            return Err(Error::new(0, "FES 文字累计超过存储上限"));
        }
        self.text.get_or_insert_with(Vec::new).extend(bytes);
        self.remaining[0] = -1;
        Ok(())
    }
}

impl Objects {
    /// 设置后续文字声明及 reset 使用的全局值，不改变既有对象的编码。
    pub fn configure_messages(
        &mut self,
        unicode: bool,
        defaults: &MessageDefaults,
        measurement: u32,
    ) {
        self.text = Context {
            unicode,
            defaults: defaults.clone(),
            measurement,
        };
    }

    fn string(&self, handle: u32) -> &StringState {
        let ObjectState::String(state) = &self.get(handle).unwrap().state else {
            unreachable!()
        };
        state
    }
    fn string_mut(&mut self, handle: u32) -> &mut StringState {
        let ObjectState::String(state) = &mut self.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        state
    }

    pub(super) fn dispatch_string_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        line: &fes::Line,
        numbers: &mut expression::State,
        frame_delay: i32,
    ) -> Result<bool> {
        let args = interpreter.parameters(&line.bytes, numbers)?;
        let Some(command) = args.get(1).map(Vec::as_slice) else {
            return Ok(false);
        };
        if !matches!(
            command,
            b"layout"
                | b"str"
                | b"strsj"
                | b"apend"
                | b"apendmark"
                | b"clear"
                | b"reset"
                | b"draw"
                | b"skip"
                | b"isdraw"
                | b"isdrawmark"
                | b"linecnt"
        ) {
            return Ok(false);
        }
        let target = interpreter.command_object_reference(&args[0], numbers)?;
        if self.fallback_missing_target(interpreter, &target, &line.bytes, numbers)? {
            return Ok(true);
        }
        let first = self.lookup(
            &target.name,
            if target.bank == -1 { 0 } else { target.bank },
        );
        if !first
            .is_some_and(|handle| matches!(self.get(handle).unwrap().state, ObjectState::String(_)))
        {
            return Ok(false);
        }
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let handles: Vec<_> = banks
            .filter_map(|bank| self.lookup(&target.name, bank as i32))
            .collect();
        let argument = if command == b"linecnt" {
            args.get(2)
                .map(|bytes| interpreter.eval(bytes, numbers))
                .transpose()?
        } else if command == b"draw" {
            Some(
                args.get(2)
                    .map_or(Ok(0), |bytes| interpreter.eval(bytes, numbers))?,
            )
        } else {
            None
        };
        if matches!(command, b"str" | b"strsj" | b"apend")
            && remainder(&line.bytes, 2).len() >= 2048
        {
            return Err(Error::new(line.offset, "FES 文字原文超过转换缓冲区容量"));
        }
        let mut layout_values = std::collections::BTreeMap::new();
        for handle in handles {
            if !matches!(self.get(handle).unwrap().state, ObjectState::String(_)) {
                continue;
            }
            let context = &self.text;
            let defaults = context.defaults.clone();
            let measurement = context.measurement;
            let state = self.string_mut(handle);
            state.layout.set_layout_defaults(&defaults);
            match command {
                b"layout" => {
                    let Some(name) = args.get(2) else { continue };
                    if name == b"font" {
                        let raw = remainder(&line.bytes, 3);
                        let text = std::str::from_utf8(raw)
                            .map_err(|_| Error::new(line.offset, "FES 字体名需要有效 UTF-8"))?;
                        state
                            .layout
                            .set_font(&text.encode_utf16().collect::<Vec<_>>(), 0)?;
                    } else if !state.layout.apply_layout(
                        name,
                        |index, default| {
                            if let Some(value) = layout_values.get(&index) {
                                return Ok(*value);
                            }
                            let value = args
                                .get(index + 3)
                                .map_or(Ok(default), |bytes| interpreter.eval(bytes, numbers))?;
                            layout_values.insert(index, value);
                            Ok(value)
                        },
                        frame_delay,
                    )? {
                        return Err(Error::new(line.offset, "FES 此类文字布局尚未接入"));
                    }
                }
                b"str" => state.set_text(remainder(&line.bytes, 2).to_vec()),
                b"strsj" => {
                    state.set_text(crate::text::cp932(remainder(&line.bytes, 2))?.into_bytes())
                }
                b"apend" => state.append(remainder(&line.bytes, 2))?,
                b"apendmark" => {
                    if state.text.is_none() {
                        return Err(Error::new(line.offset, "FES 段标记需要已初始化的文字"));
                    }
                    state.append(if state.layout.encoding() == TextEncoding::Unicode {
                        &[0xef, 0xbf, 0xbf]
                    } else {
                        &[0xff]
                    })?;
                }
                b"clear" => state.set_text(Vec::new()),
                b"reset" => state.layout.reset_layout(&defaults, measurement)?,
                b"draw" => {
                    state.draw = Some(argument.unwrap() as u32);
                    state.remaining[0] = 1;
                }
                b"skip" => state.skip = true,
                b"isdraw" => interpreter.local_values_mut()[0] = state.remaining[0],
                b"isdrawmark" => interpreter.local_values_mut()[0] = state.remaining[1],
                b"linecnt" => {
                    if let Some(index) = argument {
                        if let Some(value) = numbers.shared.get_mut(index as usize) {
                            *value = state.lines as i32;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(true)
    }
}

// 原文入口按空格数量定位，连续空格也分别计数。
fn remainder(bytes: &[u8], count: usize) -> &[u8] {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[..end];
    let start = bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b' ')
        .nth(count - 1)
        .map_or(bytes.len(), |(index, _)| index + 1);
    &bytes[start..]
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_string(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let task = objects
            .get(handle)
            .unwrap()
            .task_id
            .ok_or_else(|| Error::new(0, "FES 文字尚未完成装载准备"))?;
        let state = objects.string(handle);
        if !self
            .tree
            .node(task)
            .is_some_and(|node| node.parent == Some(owner) && node.kind == state.task_kind())
        {
            return Err(Error::new(0, "FES 下层文字任务绑定失效"));
        }
        let extra = if state.layout.encoding() == TextEncoding::Unicode {
            21
        } else {
            0
        };
        let vram = state.layout.base.vram();
        let offset = [vram[0], vram[1]].map(super::plane::integer_position);
        if state.initial || state.layout.configuration_dirty() {
            let initial = state.initial;
            let mut bytes = state.layout.submission_configuration(offset);
            self.fes_command(
                owner,
                Some(task),
                if initial { 456 } else { 457 } + extra,
                [0, 0, 1],
                Some(&mut bytes),
            )?;
            let state = objects.string_mut(handle);
            state.initial = false;
            state.layout.clear_configuration_dirty();
        }
        if objects.string(handle).clear {
            self.fes_command(owner, Some(task), 459 + extra, [0, 0, 1], None)?;
            objects.string_mut(handle).clear = false;
        }
        if objects.string(handle).replace {
            self.fes_command(owner, Some(task), 459 + extra, [0, 0, 1], None)?;
            objects.string_mut(handle).replace = false;
        }
        let state = objects.string(handle);
        if let Some(text) = &state.text {
            if text.len() != state.consumed || state.append_pending {
                let mut bytes = text[state.consumed..].to_vec();
                bytes.push(0);
                self.fes_command(owner, Some(task), 458 + extra, [0, 0, 1], Some(&mut bytes))?;
                let state = objects.string_mut(handle);
                state.consumed = state.text.as_ref().unwrap().len();
                state.append_pending = false;
                state.lines_dirty = true;
            }
        }
        if objects.string(handle).lines_dirty {
            let lines = self.string_query(owner, task, 469 + extra)?;
            let state = objects.string_mut(handle);
            state.lines = lines;
            state.lines_dirty = false;
        }
        self.update_fes_plane_properties(owner, objects, handle, Some(task))?;
        self.fes_command(owner, Some(task), 467 + extra, [0, 10, 1], None)?;
        if let Some(segment) = objects.string(handle).draw {
            self.fes_command(owner, Some(task), 463 + extra, [0, segment, 1], None)?;
            objects.string_mut(handle).draw = None;
        }
        if objects.string(handle).remaining[0] != 0 {
            let all = self.string_query(owner, task, 460 + extra)?;
            let segment = self.string_query(owner, task, 472 + extra)?;
            objects.string_mut(handle).remaining = [all as i32, segment as i32];
        }
        let mut x = [0; 4];
        let mut y = [0; 4];
        route_with_buffers(
            self.reborrow(),
            owner,
            Command {
                target: task,
                sender: owner,
                code: 466 + extra,
                data: [0, 0, 1],
            },
            &mut CommandBuffers {
                data0: Some(&mut x),
                data1: Some(&mut y),
                external: None,
            },
        )?;
        objects.string_mut(handle).cursor = [i32::from_le_bytes(x), i32::from_le_bytes(y)];
        if objects.string(handle).skip {
            self.fes_command(owner, Some(task), 461 + extra, [0, 0, 1], None)?;
            objects.string_mut(handle).skip = false;
        }
        Ok(())
    }

    fn string_query(&mut self, owner: u32, target: u32, code: u32) -> Result<u32> {
        route_with_buffers(
            self.reborrow(),
            owner,
            Command {
                target,
                sender: owner,
                code,
                data: [0, 0, 1],
            },
            &mut CommandBuffers::default(),
        )
    }
}
