use crate::{
    Error, Result, Step,
    expression::{self, State},
    read, text,
};

mod attributes;
mod events;
mod image;
mod keyblocks;
mod macros;
mod objects;
mod params;
mod plane;
mod resources;
mod subcommands;
pub use attributes::{ObjectAttributes, ObjectInitialization, ObjectReference};
pub use events::EventContext;
pub use image::{CImageState, ImageState, ImageUpdates};
pub use keyblocks::KeyBlock;
pub use objects::ObjectDefinition;
pub use params::parse as parameters;
pub use plane::{CPlaneState, PlaneState};
pub use resources::ImageReference;
use std::collections::{BTreeMap, VecDeque};
pub use subcommands::{
    MappedArgument, Subcommand, SubcommandCall, SubcommandContext, SubcommandMode,
    SubcommandParameter, SubcommandTable,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub offset: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub header: Vec<u8>,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug)]
struct Block {
    name: String,
    lines: Vec<Line>,
    false_jump: Vec<Option<usize>>,
    end_jump: Vec<Option<usize>>,
}

#[derive(Clone, Debug)]
pub struct Program {
    blocks: Vec<Block>,
    macros: macros::Macros,
    pub declarations: Vec<Declaration>,
    pub diagnostics: Vec<Error>,
}

impl Program {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.get(..4) != Some(b"FES\0") {
            return Err(Error::new(0, "无效 FES 签名"));
        }
        let c = read::sized(read::u32_at(data, 4)?, 4)?;
        let u = read::sized(read::u32_at(data, 8)?, 8)?;
        read::range(data, 0, 16)?;
        if data.len() != 16 + c {
            return Err(Error::new(4, "FES 容器长度不匹配"));
        }
        Self::from_text(&read::inflate(&data[16..], u)?)
    }
    pub fn from_text(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "FES 文本过长"));
        }
        let cleaned = comments(bytes)?;
        let mut sections: Vec<Declaration> = Vec::new();
        let mut offset = 0;
        for (number, raw) in cleaned.split_inclusive(|&b| b == b'\n').enumerate() {
            if number >= read::MAX_ITEMS {
                return Err(Error::new(offset, "FES 行数超过读取上限"));
            }
            let line = raw.trim_ascii();
            if let Some(header) = line.strip_prefix(b"#") {
                sections.push(Declaration {
                    header: header.to_vec(),
                    lines: Vec::new(),
                });
            } else if !line.is_empty() {
                let section = sections
                    .last_mut()
                    .ok_or_else(|| Error::new(offset, "FES 语句缺少头块"))?;
                let leading = raw.len() - raw.trim_ascii_start().len();
                section.lines.push(Line {
                    offset: offset + leading,
                    bytes: line.to_vec(),
                });
            }
            offset += raw.len();
        }
        let mut program = Self {
            blocks: Vec::new(),
            macros: macros::Macros::new(),
            declarations: Vec::new(),
            diagnostics: Vec::new(),
        };
        for section in sections {
            let name = section
                .header
                .split(u8::is_ascii_whitespace)
                .next()
                .unwrap_or(&[]);
            if [
                b"INCLUDE".as_slice(),
                b"PRELOAD",
                b"MACRO",
                b"DEFINE",
                b"OBJECT",
                b"KEYBLOCK",
                b"SUBCOMMAND",
            ]
            .contains(&name)
            {
                program.declarations.push(section);
                continue;
            }
            let name = text::ascii(name)?.to_owned();
            if name.is_empty() || name.len() != section.header.len() {
                return Err(Error::new(0, "执行块头需要一个名称"));
            }
            if let Some(block) = program.blocks.iter_mut().find(|b| b.name == name) {
                block.lines.extend(section.lines);
            } else {
                program.blocks.push(Block {
                    name,
                    lines: section.lines,
                    false_jump: Vec::new(),
                    end_jump: Vec::new(),
                });
            }
        }
        for section in &program.declarations {
            if section.header == b"MACRO" {
                for line in &section.lines {
                    let (name, value) = macros::declaration(&line.bytes)
                        .map_err(|error| Error::new(line.offset + error.offset, error.message))?;
                    program.macros.insert(name, value);
                }
            }
        }
        for block in &mut program.blocks {
            block.false_jump = vec![None; block.lines.len()];
            block.end_jump = block.false_jump.clone();
            let mut stack: Vec<(usize, Option<usize>)> = Vec::new();
            for (i, line) in block.lines.iter().enumerate() {
                let bytes = macros::expand(&line.bytes, &program.macros)?;
                if text::condition(&bytes)?.is_some_and(|(_, tail)| tail.is_empty()) {
                    stack.push((i, None));
                } else if bytes == b"else" {
                    if let Some((start, alt @ None)) = stack.last_mut() {
                        block.false_jump[*start] = Some(i + 1);
                        *alt = Some(i);
                    } else {
                        program
                            .diagnostics
                            .push(Error::new(line.offset, "孤立或重复 else"));
                    }
                } else if bytes == b"endif" {
                    if let Some((start, alt)) = stack.pop() {
                        if let Some(alt) = alt {
                            block.end_jump[alt] = Some(i + 1);
                        } else {
                            block.false_jump[start] = Some(i + 1);
                        }
                    } else {
                        program
                            .diagnostics
                            .push(Error::new(line.offset, "孤立 endif"));
                    }
                }
            }
            for (i, _) in stack {
                program
                    .diagnostics
                    .push(Error::new(block.lines[i].offset, "if 缺少 endif"));
            }
        }
        Ok(program)
    }

    pub fn expand_macros(&self, input: &[u8]) -> Result<Vec<u8>> {
        macros::expand(input, &self.macros)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    CreateObject(ObjectDefinition),
    InitializeObject(Box<ObjectInitialization>),
    Declarations(Vec<Declaration>),
    Command(Line),
    Subcommand(Box<SubcommandCall>),
    Wait(i32),
    Frame,
    Exit(i32),
}

pub struct Interpreter {
    program: Program,
    block: usize,
    pc: usize,
    calls: Vec<(usize, usize)>,
    local: Vec<i32>,
    pending: Option<Event>,
    ended: bool,
    pub return_value: i32,
    /// 当前 FES 的宿主命名值；对象句柄与系统值均由宿主更新。
    pub named: BTreeMap<String, BTreeMap<i32, i32>>,
    invocation: Option<events::Invocation>,
    definitions: VecDeque<Line>,
    pre_declarations: Option<Vec<Declaration>>,
    initializers: VecDeque<attributes::InitializationBlock>,
    declarations: Option<Vec<Declaration>>,
    object_banks: BTreeMap<String, u32>,
    registered_subcommands: Option<SubcommandTable>,
}

impl Interpreter {
    pub fn new(program: Program) -> Result<Self> {
        let block = program
            .blocks
            .iter()
            .position(|b| b.name == "START")
            .ok_or_else(|| Error::new(0, "FES 缺少 START 块"))?;
        let declarations: Vec<_> = program
            .declarations
            .iter()
            .filter(|d| d.header.split(u8::is_ascii_whitespace).next() == Some(b"SUBCOMMAND"))
            .cloned()
            .collect();
        let declarations = if declarations.is_empty() {
            None
        } else {
            Some(declarations)
        };
        let pre_declarations: Vec<_> = program
            .declarations
            .iter()
            .filter(|d| {
                matches!(
                    d.header.split(u8::is_ascii_whitespace).next(),
                    Some(b"INCLUDE" | b"PRELOAD" | b"KEYBLOCK")
                )
            })
            .cloned()
            .collect();
        let pre_declarations = if pre_declarations.is_empty() {
            None
        } else {
            Some(pre_declarations)
        };
        let initializers = program
            .declarations
            .iter()
            .filter(|d| d.header.split(u8::is_ascii_whitespace).next() == Some(b"OBJECT"))
            .map(|d| attributes::InitializationBlock {
                header: d.header.clone(),
                columns: None,
                lines: d.lines.iter().cloned().collect(),
            })
            .collect();
        let definitions = program
            .declarations
            .iter()
            .filter(|d| d.header == b"DEFINE")
            .flat_map(|d| d.lines.iter().cloned())
            .collect();
        Ok(Self {
            program,
            block,
            pc: 0,
            calls: Vec::new(),
            local: vec![0; 512],
            pending: None,
            ended: false,
            return_value: 0,
            named: BTreeMap::new(),
            invocation: None,
            definitions,
            pre_declarations,
            initializers,
            declarations,
            object_banks: BTreeMap::new(),
            registered_subcommands: None,
        })
    }
    pub fn resume(&mut self) {
        self.pending = None;
    }
    pub(crate) fn set_object_bank_count(&mut self, name: &str, count: u32) {
        self.object_banks.insert(name.to_owned(), count);
    }
    /// 读取已完成行级展开的命令，并展开参数内部的字符串引用。
    pub fn parameters(&self, input: &[u8], state: &State) -> Result<Vec<Vec<u8>>> {
        parameters(input)?
            .into_iter()
            .map(|s| text::expand(&s, &state.strings))
            .collect()
    }
    fn eval(&mut self, bytes: &[u8], state: &mut State) -> Result<i32> {
        let current = self
            .invocation
            .as_ref()
            .map_or(state.current, |invocation| invocation.context.bank);
        self.eval_with_current(bytes, state, current)
    }
    fn eval_with_current(&mut self, bytes: &[u8], state: &mut State, value: i32) -> Result<i32> {
        let current = state.current;
        state.current = value;
        std::mem::swap(&mut self.local, &mut state.local);
        std::mem::swap(&mut self.named, &mut state.named);
        let result = text::ascii(bytes).and_then(|s| expression::eval(s, state));
        std::mem::swap(&mut self.named, &mut state.named);
        std::mem::swap(&mut self.local, &mut state.local);
        state.current = current;
        result
    }
    pub fn run(&mut self, state: &mut State, budget: usize) -> Result<Step<Event>> {
        if let Some(event) = &self.pending {
            return Ok(Step::Event(event.clone()));
        }
        if self.ended {
            return Ok(Step::Halted);
        }
        for _ in 0..budget {
            if let Some(declarations) = self.pre_declarations.take() {
                let event = Event::Declarations(declarations);
                self.pending = Some(event.clone());
                return Ok(Step::Event(event));
            }
            if let Some(line) = self.definitions.pop_front() {
                let event = self.define_object(&line, state)?;
                self.pending = Some(event.clone());
                return Ok(Step::Event(event));
            }
            if let Some(block) = self.initializers.front() {
                if block.columns.is_none() {
                    let header = block.header.clone();
                    let columns = self.initialization_columns(&header, state)?;
                    self.initializers.front_mut().unwrap().columns = Some(columns);
                }
                let block = self.initializers.front_mut().unwrap();
                if let Some(line) = block.lines.pop_front() {
                    let columns = block.columns.as_ref().unwrap().clone();
                    let initialization = self
                        .initialize_object(&line, &columns, state)
                        .map_err(|error| Error::new(line.offset + error.offset, error.message))?;
                    if initialization.attributes == ObjectAttributes::default() {
                        continue;
                    }
                    let event = Event::InitializeObject(Box::new(initialization));
                    self.pending = Some(event.clone());
                    return Ok(Step::Event(event));
                }
                self.initializers.pop_front();
                continue;
            }
            if let Some(declarations) = self.declarations.take() {
                self.registered_subcommands = Some(self.subcommands(state));
                let event = Event::Declarations(declarations);
                self.pending = Some(event.clone());
                return Ok(Step::Event(event));
            }
            let Some(line) = self.program.blocks[self.block].lines.get(self.pc).cloned() else {
                if self.invocation.is_some() {
                    if self.event_block_ended() {
                        return Ok(Step::Yield);
                    }
                    continue;
                }
                if let Some((block, pc)) = self.calls.pop() {
                    self.block = block;
                    self.pc = pc;
                    continue;
                }
                self.pc = 0;
                self.pending = Some(Event::Frame);
                return Ok(Step::Event(Event::Frame));
            };
            let i = self.pc;
            self.pc += 1;
            if let Some(error) = self
                .program
                .diagnostics
                .iter()
                .find(|e| e.offset == line.offset)
            {
                return Err(error.clone());
            }
            let expanded = text::expand(&line.bytes, &state.strings)?;
            let expanded = self.program.expand_macros(&expanded)?;
            let mut bytes = expanded.as_slice();
            if bytes.trim_ascii().is_empty() {
                continue;
            }
            if let Some((condition, tail)) = text::condition(bytes)? {
                let yes = self.eval(condition, state)? != 0;
                if tail.is_empty() {
                    if !yes {
                        self.pc = self.program.blocks[self.block].false_jump[i]
                            .ok_or_else(|| Error::new(line.offset, "条件结构不完整"))?;
                    }
                    continue;
                }
                if !yes {
                    continue;
                }
                bytes = tail;
            }
            if bytes == b"else" {
                self.pc = self.program.blocks[self.block].end_jump[i]
                    .ok_or_else(|| Error::new(line.offset, "else 结构不完整"))?;
                continue;
            }
            if bytes == b"endif" {
                continue;
            }
            if bytes.first().is_some_and(|b| matches!(b, b'$' | b'\\')) {
                self.eval(bytes, state)?;
                continue;
            }
            let args = self.parameters(bytes, state)?;
            let Some(command) = args.first().map(Vec::as_slice) else {
                continue;
            };
            if self.invocation.is_some() && matches!(command, b"wait" | b"exit") {
                return Err(Error::new(line.offset, "FES 事件内的 wait/exit 尚不支持"));
            }
            let event = match command {
                b"next" | b"call" => {
                    if args.len() != 2 {
                        return Err(Error::new(line.offset, "块转移需要一个目标"));
                    }
                    let target = text::ascii(&args[1])?;
                    let block = self
                        .program
                        .blocks
                        .iter()
                        .position(|b| b.name == target)
                        .ok_or_else(|| Error::new(line.offset, "目标块不存在"))?;
                    if command == b"call" {
                        if self.calls.len() >= 128 {
                            return Err(Error::new(line.offset, "FES 调用过深"));
                        }
                        self.calls.push((self.block, self.pc));
                    } else {
                        self.calls.clear();
                    }
                    self.block = block;
                    self.pc = 0;
                    if self.event_transferred() {
                        return Ok(Step::Yield);
                    }
                    continue;
                }
                b"break" => {
                    self.return_value = if args.len() == 1 {
                        0
                    } else {
                        self.eval(&args[1], state)?
                    };
                    self.pc = self.program.blocks[self.block].lines.len();
                    continue;
                }
                b"exit" => {
                    let code = if args.len() == 1 {
                        0
                    } else {
                        self.eval(&args[1], state)?
                    };
                    self.ended = true;
                    Event::Exit(code)
                }
                b"wait"
                    if args.len() > 1
                        && args[1].first().is_some_and(|b| {
                            b.is_ascii_digit()
                                || matches!(b, b'$' | b'\\' | b'(' | b'@' | b'-' | b'+')
                        }) =>
                {
                    Event::Wait(self.eval(&args[1], state)?)
                }
                _ => match self
                    .subcommand_call(line.offset, &args, state)
                    .map_err(|error| Error::new(line.offset + error.offset, error.message))?
                {
                    Some(call) => Event::Subcommand(Box::new(call)),
                    None => Event::Command(Line {
                        offset: line.offset,
                        bytes: bytes.to_vec(),
                    }),
                },
            };
            self.pending = Some(event.clone());
            return Ok(Step::Event(event));
        }
        Ok(Step::Yield)
    }
}

fn comments(input: &[u8]) -> Result<Vec<u8>> {
    let mut out = input.to_vec();
    let mut p = 0;
    let mut block = false;
    let mut quote = None;
    while p < input.len() {
        let b = input[p];
        if block {
            if input[p..].starts_with(b"*/") {
                out[p..p + 2].fill(b' ');
                p += 2;
                block = false;
            } else {
                if b != b'\n' && b != b'\r' {
                    out[p] = b' ';
                }
                p += 1;
            }
        } else if matches!(b, 0x81..=0x9f | 0xe0..=0xfc) && p + 1 < input.len() {
            p += 2;
        } else if let Some(delimiter) = quote {
            if b == b'\\'
                && input
                    .get(p + 1)
                    .is_some_and(|&next| next == delimiter || (delimiter == b'\'' && next == b'\\'))
            {
                p += 2;
            } else {
                if b == delimiter {
                    quote = None;
                }
                p += 1;
            }
        } else if matches!(b, b'"' | b'\'') {
            quote = Some(b);
            p += 1;
        } else if input[p..].starts_with(b"/*") {
            out[p..p + 2].fill(b' ');
            p += 2;
            block = true;
        } else if input[p..].starts_with(b"//") {
            while p < input.len() && input[p] != b'\n' {
                out[p] = b' ';
                p += 1;
            }
        } else {
            p += 1;
        }
    }
    if block {
        return Err(Error::new(p, "块注释未闭合"));
    }
    if quote.is_some() {
        return Err(Error::new(p, "引号未闭合"));
    }
    Ok(out)
}
