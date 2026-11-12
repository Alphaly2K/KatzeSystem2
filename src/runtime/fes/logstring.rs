use super::{ObjectState, Objects, Scene, StringState, string};
use crate::runtime::{ObjectFactory, Store, create_object};
use crate::{Error, Result, backlog::Backlog, fes};

#[derive(Clone, Debug, PartialEq)]
struct Row {
    task: u32,
    text: StringState,
    secondary: Option<(u32, StringState)>,
}

/// 日志文字的可见行、当前位置及各行独立的下层文字任务。
#[derive(Clone, Debug, PartialEq)]
pub struct LogStringState {
    pub(super) template: StringState,
    kind: &'static str,
    log: u32,
    count: usize,
    position: i32,
    drawing: bool,
    redraw: bool,
    rows: Vec<Row>,
    positions: Vec<[f32; 2]>,
    geometry_dirty: bool,
    subtitle: bool,
    subtitle_offset: [i32; 2],
    colors: [[u32; 3]; 6],
    row_colors: Vec<usize>,
}

impl LogStringState {
    pub(super) fn new(kind: &str, context: &string::Context) -> Result<Self> {
        Ok(Self {
            template: StringState::new(
                if kind == "LOGSTRING8" {
                    "STRING8"
                } else {
                    "STRING"
                },
                context,
            )?,
            kind: if kind == "LOGSTRING8" {
                "LOGSTRING8"
            } else {
                "LOGSTRING"
            },
            log: 0,
            count: 3,
            position: 0,
            drawing: false,
            redraw: true,
            rows: Vec::new(),
            positions: Vec::new(),
            geometry_dirty: true,
            subtitle: false,
            subtitle_offset: [0; 2],
            colors: [[u32::MAX, 0xff00_0000, u32::MAX]; 6],
            row_colors: vec![0; 3],
        })
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }
    pub(super) fn task_display(&self, task: u32) -> bool {
        self.drawing && (self.subtitle || self.rows.iter().any(|row| row.task == task))
    }
    pub fn position(&self) -> i32 {
        self.position
    }
    pub fn visible_rows(&self) -> usize {
        self.count
    }
    pub fn log_handle(&self) -> u32 {
        self.log
    }
    pub fn task_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.rows.iter().flat_map(|row| {
            std::iter::once(row.task).chain(row.secondary.as_ref().map(|(task, _)| *task))
        })
    }

    pub(super) fn apply(&mut self, attributes: &fes::ObjectAttributes, bank: u32) -> Result<bool> {
        self.geometry_dirty |= attributes.position_resource.is_some()
            || attributes.position.iter().any(Option::is_some);
        if let Some(count) = attributes.count {
            self.set_count(count)?;
        }
        if let Some(display) = attributes.display {
            self.drawing = display != 0;
            self.redraw = true;
        }
        Ok(self.template.layout.base.apply(attributes, bank))
    }

    fn set_count(&mut self, count: i32) -> Result<()> {
        if count < 0 || count as usize > crate::read::MAX_ITEMS {
            return Err(Error::new(0, "FES 日志可见行数超过存储上限"));
        }
        self.count = count as usize;
        self.geometry_dirty = true;
        self.row_colors.resize(self.count, 0);
        self.redraw = true;
        Ok(())
    }

    fn bind(&mut self, handle: u32, log: Option<&Backlog>) {
        self.log = handle;
        if let Some(log) = log {
            self.position = log.row_count().saturating_sub(self.count) as i32;
        }
        self.redraw = true;
    }

    fn set_position(&mut self, position: i32, log: Option<&Backlog>, relative: bool) {
        let Some(log) = log else { return };
        let end = log.row_count() as i32 - self.count as i32;
        let next = position.max(0).min(end.max(0));
        // 相对滚动在日志不足一屏时不改变位置。
        if relative && end < 0 {
            return;
        }
        if !relative || self.position != next {
            self.position = next;
            self.row_colors.fill(0);
            self.redraw = true;
        }
    }

    fn color_block(&mut self, row: i32, color: usize, log: Option<&Backlog>) {
        if row < 0 || row as usize >= self.count {
            return;
        }
        let Some(info) = log.and_then(|log| log.row_info(self.position.wrapping_add(row) as u32))
        else {
            return;
        };
        for (i, selected) in self.row_colors.iter_mut().enumerate() {
            let index = self.position.wrapping_add(i as i32) as u32;
            if index >= info.first && index <= info.last {
                *selected = if matches!(color, 1 | 2) { color } else { 0 };
            }
        }
        self.redraw = true;
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_log_command(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        let Some(command) = args.get(1).map(Vec::as_slice) else {
            return Ok(false);
        };
        if !matches!(
            command,
            b"setlog"
                | b"remake"
                | b"max"
                | b"getpos"
                | b"pos"
                | b"page"
                | b"next"
                | b"rev"
                | b"draw"
                | b"disp"
                | b"pathinfo"
                | b"lineinfo"
                | b"blockinfo"
                | b"blockcolor"
                | b"linecolor"
                | b"layout"
        ) {
            return Ok(false);
        }
        let color = if command == b"layout" {
            match args.get(2).map(Vec::as_slice) {
                Some(b"color") => Some(0),
                Some(b"vcolor") => Some(1),
                Some(b"acolor") => Some(2),
                Some(b"vacolor") => Some(3),
                Some(b"pcolor") => Some(4),
                Some(b"vpcolor") => Some(5),
                Some(b"shift_sub") => None,
                _ => return Ok(false),
            }
        } else {
            None
        };
        let name = if args[0] == b"@" {
            scene
                .interpreter
                .event_context()
                .map(|context| context.object.as_str())
        } else {
            let end = args[0]
                .iter()
                .position(|&byte| matches!(byte, b'[' | b' '))
                .unwrap_or(args[0].len());
            std::str::from_utf8(&args[0][..end]).ok()
        };
        if !name
            .and_then(|name| scene.objects.lookup(name, 0))
            .is_some_and(|handle| {
                matches!(
                    scene.objects.get(handle).unwrap().state,
                    ObjectState::LogString(_)
                )
            })
        {
            return Ok(false);
        }
        let mut target = scene
            .interpreter
            .command_object_reference(&args[0], self.numbers)?;
        let first = scene
            .objects
            .lookup(&target.name, if target.bank < 0 { 0 } else { target.bank });
        if !first.is_some_and(|handle| {
            matches!(
                scene.objects.get(handle).unwrap().state,
                ObjectState::LogString(_)
            )
        }) {
            return Ok(false);
        }
        if scene.objects.count(&target.name) == 1 {
            target.bank = 0;
        }
        let start = if command == b"layout" { 3 } else { 2 };
        let defaults: &[i32] = match command {
            b"layout" if color.is_some() => &[0; 9],
            b"layout" => &[0, 0],
            b"blockinfo" => &[0, -1, -1, -1, -1],
            b"pathinfo" | b"lineinfo" => &[0, 0, 0],
            b"getpos" => &[-1, -1, -1],
            b"max" => &[-1],
            b"next" | b"disp" => &[1],
            b"rev" => &[-1],
            b"remake" | b"blockcolor" => &[0, 0],
            b"linecolor" => &[0, -1, -1, -1],
            b"setlog" | b"pos" | b"page" => &[0],
            _ => &[],
        };
        let mut values = Vec::with_capacity(defaults.len());
        for (i, default) in defaults.iter().enumerate() {
            values.push(args.get(start + i).map_or(Ok(*default), |bytes| {
                scene.interpreter.eval(bytes, self.numbers)
            })?);
        }
        let banks = if target.bank == -1 {
            0..i64::from(scene.objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = scene.objects.lookup(&target.name, bank as i32) else {
                continue;
            };
            let ObjectState::LogString(state) = &mut scene.objects.get_mut(handle).unwrap().state
            else {
                continue;
            };
            let log = if state.log == 0 {
                None
            } else {
                Some(
                    self.resources
                        .backlogs
                        .get(&state.log)
                        .ok_or_else(|| Error::new(line.offset, "FES 日志句柄无效"))?,
                )
            };
            match command {
                b"setlog" => {
                    let handle = values[0] as u32;
                    let log = if handle == 0 {
                        None
                    } else {
                        Some(
                            self.resources
                                .backlogs
                                .get(&handle)
                                .ok_or_else(|| Error::new(line.offset, "FES 日志句柄无效"))?,
                        )
                    };
                    state.bind(handle, log);
                }
                b"remake" => {
                    if state.template.layout.encoding() == crate::message::TextEncoding::Unicode {
                        let primary = args
                            .get(2)
                            .and_then(|_| self.host.languages.prefixes.get(values[0] as usize))
                            .cloned();
                        let subtitle = args
                            .get(3)
                            .and_then(|_| self.host.languages.prefixes.get(values[1] as usize));
                        let subtitle = subtitle.cloned();
                        self.remake_fes_log(state.log, [primary.as_deref(), subtitle.as_deref()])?;
                        state.subtitle = subtitle.is_some();
                        state.redraw = true;
                    }
                }
                b"max" => write(
                    self.numbers,
                    values[0],
                    log.map_or(0, |log| log.row_count() as i32),
                ),
                b"getpos" => {
                    if target.bank != -1 {
                        for (slot, value) in values.iter().zip([state.position, 0, 0]) {
                            write(self.numbers, *slot, value);
                        }
                    }
                }
                b"pos" | b"page" => {
                    let position = if command == b"page" {
                        state
                            .position
                            .wrapping_add(values[0].wrapping_mul(state.count as i32))
                    } else {
                        values[0]
                    };
                    state.set_position(position, log, false);
                }
                b"next" | b"rev" => {
                    state.set_position(state.position.wrapping_add(values[0]), log, true)
                }
                b"draw" => {
                    state.row_colors.fill(0);
                    state.redraw = true;
                    state.drawing = true;
                }
                b"disp" => {
                    state.drawing = values[0] != 0;
                    state.redraw = true;
                }
                b"blockcolor" => state.color_block(values[0], values[1] as usize, log),
                b"linecolor" => {
                    if let Some(row) = state.rows.get_mut(values[0] as usize) {
                        let mut colors = row.text.layout.restore_colors();
                        for (color, value) in colors.iter_mut().zip(&values[1..]) {
                            if *value != -1 {
                                *color = *value as u32;
                            }
                        }
                        row.text.layout.set_colors(colors);
                        if let Some(text) = row.text.text() {
                            row.text.set_log_text(text.to_vec());
                        }
                    }
                }
                b"lineinfo" => {
                    let info = log.and_then(|log| log.row_info(values[0] as u32));
                    self.numbers.strings.insert(
                        values[1] as u32,
                        info.as_ref()
                            .map_or_else(Vec::new, |info| info.file.to_vec()),
                    );
                    write(
                        self.numbers,
                        values[2],
                        info.map_or(-1, |info| info.input as i32),
                    );
                }
                b"pathinfo" => {
                    if let Some(info) = log.and_then(|log| log.row_info(values[0] as u32)) {
                        write(self.numbers, values[1], info.extra as i32);
                        write(self.numbers, values[2], info.input as i32);
                    }
                }
                b"blockinfo" => {
                    let info = log.and_then(|log| log.row_info(values[0] as u32));
                    let result = info.map_or([-1, -1, -1, 0], |info| {
                        [
                            info.group as i32,
                            info.first as i32,
                            info.last as i32,
                            (info.flags & 4) as i32,
                        ]
                    });
                    for (&slot, value) in values[1..].iter().zip(result) {
                        write(self.numbers, slot, value);
                    }
                }
                b"layout" => {
                    if let Some(index) = color {
                        let mut colors = [0xff00_0000; 3];
                        for (i, value) in values.iter().enumerate() {
                            colors[i / 3] |= (*value as u8 as u32) << ((i % 3) * 8);
                        }
                        state.colors[index] = colors;
                    } else {
                        state.subtitle_offset = [values[0], values[1]];
                    }
                    state.redraw = true;
                }
                _ => unreachable!(),
            }
        }
        Ok(true)
    }

    fn remake_fes_log(&mut self, handle: u32, languages: [Option<&[u8]>; 2]) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let sources = self
            .resources
            .backlogs
            .get(&handle)
            .ok_or_else(|| Error::new(0, "FES 日志句柄无效"))?
            .group_sources();
        if sources.is_empty() {
            return Ok(());
        }
        let registered = languages
            .iter()
            .flatten()
            .map(|text| text.to_vec())
            .collect::<Vec<_>>();
        let mut scene = crate::runtime::CstContext::new(&registered);
        let mut previous = None;
        let mut loaded = false;
        for (group, file, input) in sources {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 CST 资源服务"))?;
            if previous.as_ref() != Some(&file) {
                let resolved = services.resolve(b"scene", &file)?;
                loaded = scene.load(&resolved, services)?;
                previous = Some(file);
            }
            let records = loaded
                .then(|| scene.script())
                .flatten()
                .and_then(|script| {
                    script.inputs().get(input as usize).map(|range| {
                        script.records()[range.clone()]
                            .iter()
                            .map(|record| record.kind)
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
            let mut body = [Vec::new(), Vec::new()];
            let mut name = None;
            for (index, kind) in records.into_iter().enumerate() {
                if !matches!(kind, 0x20 | 0x21) {
                    continue;
                }
                for (language_index, language) in languages.iter().enumerate() {
                    if language_index == 1 && language.is_none() {
                        continue;
                    }
                    let text = scene
                        .record_text(
                            input as i32,
                            index as i32,
                            *language,
                            true,
                            self.host.unicode_text != 0,
                        )?
                        .unwrap_or_default();
                    if kind == 0x21 {
                        if language_index == 0 && language.is_some() {
                            let marker = "＠".as_bytes();
                            let start = text
                                .windows(marker.len())
                                .position(|bytes| bytes == marker)
                                .map_or(0, |at| at + marker.len());
                            name = Some(text[start..].to_vec());
                        }
                    } else {
                        let mut text = text.as_slice();
                        while text.starts_with(b"\\r") {
                            text = &text[2..];
                        }
                        if body[language_index].len().saturating_add(text.len())
                            >= crate::read::MAX_BYTES
                        {
                            return Err(Error::new(0, "FES 日志重建文字超过存储上限"));
                        }
                        body[language_index].extend_from_slice(text);
                    }
                }
            }
            self.resources
                .backlogs
                .get_mut(&handle)
                .unwrap()
                .replace_group_text(group, &body[0], &body[1], name.as_deref())?;
        }
        Ok(())
    }

    pub(super) fn check_fes_log_binding(&self, owner: u32, state: &LogStringState) -> Result<()> {
        if state.task_ids().any(|task| {
            !self.tree.node(task).is_some_and(|node| {
                node.parent == Some(owner) && node.kind == state.template.task_kind()
            })
        }) {
            return Err(Error::new(0, "FES 下层日志文字任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn update_fes_log(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let object = objects.get(handle).unwrap();
        let ObjectState::LogString(state) = &object.state else {
            unreachable!()
        };
        self.check_fes_log_binding(owner, state)?;
        let source = self.fes_user_font_source(objects, &state.template)?;
        let layout = &state.template.layout;
        let size = if layout.base.base.rebuild {
            match &layout.base.base.size_resource {
                Some(reference) => {
                    self.fes_image_geometry(&objects.preloaded_images, reference, true)?
                }
                None => layout.base.base.size,
            }
        } else {
            layout.base.base.size
        };
        let mut positions = state.positions.clone();
        if state.geometry_dirty || positions.len() != state.count {
            positions.clear();
            for i in 0..state.count {
                let position = if let Some(reference) = &layout.base.base.position_resource {
                    let mut reference = reference.clone();
                    reference.frame = reference.frame.wrapping_add(i as i32);
                    self.fes_image_geometry(&objects.preloaded_images, &reference, false)?
                        .map(|value| value as f32)
                } else {
                    layout.base.base.position[..2].try_into().unwrap()
                };
                positions.push(position);
            }
        }
        let object = objects.get_mut(handle).unwrap();
        let ObjectState::LogString(state) = &mut object.state else {
            unreachable!()
        };
        if state.rows.len() != state.count {
            for row in state.rows.drain(..) {
                for task in std::iter::once(row.task).chain(row.secondary.map(|(task, _)| task)) {
                    self.fes_command(owner, Some(task), 3, [0; 3], None)?;
                    object.input_tasks.retain(|&saved| saved != task);
                }
            }
            for _ in 0..state.count {
                let task = create_object(
                    self.reborrow(),
                    owner,
                    state.template.task_kind(),
                    b"FE log string",
                    b"",
                )?;
                let mut text = state.template.clone();
                text.initial = true;
                state.rows.push(Row {
                    task,
                    text,
                    secondary: None,
                });
                object.input_tasks.push(task);
            }
            state.redraw = true;
        }
        let layout_dirty = state.template.layout.configuration_dirty()
            || state.template.layout.shift_dirty()
            || state.template.layout.base.base.rebuild;
        let redraw = state.redraw || layout_dirty;
        let glyphs_dirty = state.template.user_fonts_dirty;
        let log = if state.log == 0 {
            None
        } else {
            Some(
                self.resources
                    .backlogs
                    .get(&state.log)
                    .ok_or_else(|| Error::new(0, "FES 日志句柄无效"))?,
            )
        };
        let content: Vec<_> = (0..state.count)
            .map(|i| {
                let index = state.position.wrapping_add(i as i32) as u32;
                let voice = log
                    .and_then(|log| log.row_info(index))
                    .is_some_and(|info| info.flags & 4 != 0);
                (
                    if redraw && state.drawing {
                        log.and_then(|log| log.row(index)).map(<[u8]>::to_vec)
                    } else {
                        None
                    },
                    voice,
                    if redraw && state.drawing && state.subtitle {
                        log.and_then(|log| log.secondary_row(index))
                            .map(<[u8]>::to_vec)
                    } else {
                        None
                    },
                )
            })
            .collect();
        let vram = state.template.layout.base.vram();
        for (i, row) in state.rows.iter_mut().enumerate() {
            if state.subtitle && row.secondary.is_none() {
                let task = create_object(
                    self.reborrow(),
                    owner,
                    state.template.task_kind(),
                    b"FE log subtitle",
                    b"",
                )?;
                let mut text = state.template.clone();
                text.initial = true;
                row.secondary = Some((task, text));
                object.input_tasks.push(task);
            }
            let colors = state.colors[state.row_colors[i] * 2 + usize::from(content[i].1)];
            let mut cursor = row.text.cursor();
            for secondary in [false, true] {
                let (task, text) = if secondary {
                    let Some((task, text)) = row.secondary.as_mut() else {
                        continue;
                    };
                    (*task, text)
                } else {
                    (row.task, &mut row.text)
                };
                let initial = text.initial;
                if layout_dirty || initial {
                    text.layout = state.template.layout.clone();
                    text.layout.set_view_size(size);
                    text.layout.base.base.rebuild = true;
                }
                text.layout.base.apply(
                    &fes::ObjectAttributes {
                        vram: Some([
                            (vram[0] as i32).wrapping_add(if secondary { size[0] } else { 0 }),
                            (vram[1] as i32)
                                .wrapping_add((size[1].wrapping_add(2)).wrapping_mul(i as i32)),
                        ]),
                        ..Default::default()
                    },
                    0,
                );
                if glyphs_dirty || initial {
                    text.user_font_source = state.template.user_font_source;
                    text.user_fonts.clone_from(&state.template.user_fonts);
                    text.user_fonts_dirty = true;
                }
                if (redraw || initial) && state.drawing && state.log != 0 {
                    text.layout.set_colors(colors);
                    text.set_log_text(
                        if secondary {
                            content[i].2.clone()
                        } else {
                            content[i].0.clone()
                        }
                        .unwrap_or_default(),
                    );
                }
                state.template.copy_layout_requests(text);
                self.submit_fes_string(owner, task, text, source)?;
                if redraw
                    || initial
                    || state.template.layout.base.base.dirty
                    || text.layout.base.base.dirty
                {
                    let position = if secondary {
                        [
                            positions[i][0] + state.subtitle_offset[0] as f32,
                            positions[i][1] + state.subtitle_offset[1] as f32 + cursor[1] as f32,
                        ]
                    } else {
                        positions[i]
                    };
                    for (code, source, values) in [
                        (51, 0, [position[0], position[1], 0.0]),
                        (
                            49,
                            0,
                            [0.0, 0.0, state.template.layout.base.base.priority as f32],
                        ),
                        (51, 1, text.layout.base.vram()[..3].try_into().unwrap()),
                    ] {
                        let mut bytes = values.map(f32::to_le_bytes).concat();
                        self.fes_command(
                            owner,
                            Some(task),
                            code,
                            [source, 0, 1],
                            Some(&mut bytes),
                        )?;
                    }
                    self.fes_command(
                        owner,
                        Some(task),
                        47,
                        [
                            0,
                            u32::from(state.drawing && (!secondary || state.subtitle)),
                            1,
                        ],
                        None,
                    )?;
                    text.layout.base.base.dirty = false;
                }
                self.finish_fes_string(owner, task, text)?;
                if !secondary {
                    cursor = text.cursor();
                }
            }
        }
        state.positions = positions;
        state.geometry_dirty = false;
        state.template.layout.base.base.size = size;
        state.template.layout.base.base.rebuild = false;
        state.template.layout.base.base.dirty = false;
        state.template.layout.clear_configuration_dirty();
        state.template.user_fonts_dirty = false;
        state.template.clear_layout_requests();
        state.redraw = false;
        Ok(())
    }
}

fn write(numbers: &mut crate::expression::State, index: i32, value: i32) {
    if let Some(slot) = numbers.shared.get_mut(index as usize) {
        *slot = value;
    }
}

#[cfg(test)]
mod tests;
