use crate::{
    Error, Result, Step, cstl,
    expression::{self, State},
    read, text,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Record {
    pub kind: u8,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Script {
    records: Vec<Record>,
    inputs: Vec<std::ops::Range<usize>>,
}

impl Script {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.get(..8) != Some(b"CatScene") {
            return Err(Error::new(0, "无效 CST 签名"));
        }
        let c = read::sized(read::u32_at(data, 8)?, 8)?;
        let u = read::sized(read::u32_at(data, 12)?, 12)?;
        let bytes = if c == 0 {
            read::range(data, 16, u)?.to_vec()
        } else {
            read::inflate(read::range(data, 16, c)?, u)?
        };
        if data.len() != 16 + if c == 0 { u } else { c } {
            return Err(Error::new(16, "CST 容器长度不匹配"));
        }
        let payload = read::sized(read::u32_at(&bytes, 0)?, 0)?;
        if payload + 16 != bytes.len() {
            return Err(Error::new(0, "CST 主体长度不匹配"));
        }
        let n = read::u32_at(&bytes, 4)? as usize;
        let o = read::sized(read::u32_at(&bytes, 8)?, 8)?;
        let s = read::sized(read::u32_at(&bytes, 12)?, 12)?;
        if n > payload / 8 || o < n * 8 || s < o || s > payload || (s - o) % 4 != 0 {
            return Err(Error::new(4, "CST 表范围无效"));
        }
        let count = (s - o) / 4;
        if count > read::MAX_ITEMS {
            return Err(Error::new(8, "CST 记录数量超过上限"));
        }
        let mut records = Vec::new();
        for i in 0..count {
            let offset = read::u32_at(&bytes, 16 + o + i * 4)? as usize;
            let raw = read::zstr(bytes.get(16 + s..).unwrap(), offset)?;
            if raw.len() < 2 || raw[0] != 1 {
                return Err(Error::new(16 + s + offset, "无效 CST 记录前缀"));
            }
            records.push(Record {
                kind: raw[1],
                bytes: raw[2..].to_vec(),
            });
        }
        let mut inputs = Vec::new();
        let mut previous = 0;
        for i in 0..n {
            let length = read::u32_at(&bytes, 16 + i * 8)? as usize;
            let start = read::u32_at(&bytes, 20 + i * 8)? as usize;
            let end = start
                .checked_add(length)
                .filter(|&x| x <= count)
                .ok_or_else(|| Error::new(16 + i * 8, "输入区间越界"))?;
            if start != previous {
                return Err(Error::new(16 + i * 8, "输入区间不连续"));
            }
            inputs.push(start..end);
            previous = end;
        }
        if previous != count {
            return Err(Error::new(4, "输入区间未覆盖全部记录"));
        }
        Ok(Self { records, inputs })
    }
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    pub fn inputs(&self) -> &[std::ops::Range<usize>] {
        &self.inputs
    }

    /// 查询输入段内的记录类型；负输入段查询段数，负记录查询该段记录数。
    /// 非负索引越界时返回 u32::MAX。
    pub fn record_kind(&self, input: i32, record: i32) -> u32 {
        if input < 0 {
            return self.inputs.len() as u32;
        }
        let Some(range) = self.inputs.get(input as usize) else {
            return u32::MAX;
        };
        if record < 0 {
            return range.len() as u32;
        }
        if record as usize >= range.len() {
            return u32::MAX;
        }
        self.records[range.start + record as usize].kind as u32
    }
}

#[derive(Clone)]
pub struct Scene {
    pub script: Arc<Script>,
    pub localization: Option<Arc<cstl::Table>>,
}

pub trait SceneSource {
    fn load(&mut self, name: &str) -> Result<Scene>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: i32,
    pub target: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Name {
        base: String,
        localized: Option<String>,
    },
    Text {
        base: String,
        localized: Option<String>,
    },
    Input {
        page: bool,
    },
    Wait(i32),
    Choices {
        interface: Option<String>,
        choices: Vec<Choice>,
    },
    Command(Vec<Vec<u8>>),
}

struct Frame {
    scene: Scene,
    pc: usize,
}

pub struct Interpreter {
    frames: Vec<Frame>,
    pending: Option<Event>,
    pub language: String,
}

impl Interpreter {
    pub fn new(scene: Scene, language: impl Into<String>) -> Self {
        Self {
            frames: vec![Frame { scene, pc: 0 }],
            pending: None,
            language: language.into(),
        }
    }
    /// 姓名、正文、输入、等待和宿主命令完成后确认，随后才推进下一记录。
    pub fn resume(&mut self) -> Result<()> {
        if matches!(self.pending, Some(Event::Choices { .. })) {
            return Err(Error::new(0, "选项需要通过 choose 确认"));
        }
        self.pending = None;
        Ok(())
    }
    pub fn choose(&mut self, index: usize, source: &mut impl SceneSource) -> Result<()> {
        let Some(Event::Choices { choices, .. }) = &self.pending else {
            return Err(Error::new(0, "当前没有待选菜单"));
        };
        let choice = choices
            .get(index)
            .ok_or_else(|| Error::new(index, "选项编号越界"))?;
        let scene = source.load(&choice.target)?;
        let frame = self.frames.last_mut().unwrap();
        frame.scene = scene;
        frame.pc = 0;
        self.pending = None;
        Ok(())
    }
    pub fn run(
        &mut self,
        state: &mut State,
        source: &mut impl SceneSource,
        budget: usize,
    ) -> Result<Step<Event>> {
        if let Some(event) = &self.pending {
            return Ok(Step::Event(event.clone()));
        }
        for _ in 0..budget {
            let Some(frame) = self.frames.last_mut() else {
                return Ok(Step::Halted);
            };
            let Some(record) = frame.scene.script.records.get(frame.pc).cloned() else {
                self.frames.pop();
                continue;
            };
            let pc = frame.pc;
            frame.pc += 1;
            let mut bytes = text::expand(&record.bytes, &state.strings)?;
            let event = match record.kind {
                0x20 | 0x21 => {
                    let group = frame
                        .scene
                        .script
                        .inputs
                        .iter()
                        .position(|g| g.contains(&pc))
                        .unwrap();
                    let range = &frame.scene.script.inputs[group];
                    let fragment = frame.scene.script.records[range.start..pc]
                        .iter()
                        .filter(|r| r.kind == 0x20)
                        .count();
                    let table = frame.scene.localization.as_ref();
                    let localized = if record.kind == 0x21 {
                        table
                            .and_then(|t| t.message(&self.language, group))
                            .map(|m| m.name.clone())
                    } else {
                        table
                            .and_then(|t| t.segment(&self.language, group, fragment as i32))
                            .map(str::to_owned)
                    };
                    let base = text::cp932(&bytes)?;
                    if record.kind == 0x21 {
                        Event::Name { base, localized }
                    } else {
                        Event::Text { base, localized }
                    }
                }
                0x02 | 0x03 => Event::Input {
                    page: record.kind == 0x03,
                },
                0xf0 | 0xf1 => continue,
                0x30 => {
                    bytes = command_bytes(&bytes);
                    let Some(command) = conditional(&bytes, state)? else {
                        continue;
                    };
                    if command.starts_with(b"#") {
                        expression::eval(text::ascii(command)?, state)?;
                        continue;
                    }
                    let args = text::arguments(command)?;
                    let Some(head) = args.first() else {
                        continue;
                    };
                    match head.as_slice() {
                        b"next" | b"call" => {
                            if args.len() != 2 {
                                return Err(Error::new(pc, "场景转移需要一个目标"));
                            }
                            let scene = source.load(&text::cp932(&args[1])?)?;
                            if head == b"call" {
                                if self.frames.len() >= 128 {
                                    return Err(Error::new(pc, "场景调用过深"));
                                }
                                self.frames.push(Frame { scene, pc: 0 });
                            } else {
                                let frame = self.frames.last_mut().unwrap();
                                frame.scene = scene;
                                frame.pc = 0;
                            }
                            continue;
                        }
                        b"return" => {
                            if args.len() != 1 {
                                return Err(Error::new(pc, "return 不接受参数"));
                            }
                            self.frames.pop();
                            continue;
                        }
                        b"str" | b"strn" => {
                            if args.len() != 3 {
                                return Err(Error::new(pc, "字符串命令需要编号和值"));
                            }
                            let index = expression::eval_parameter(text::ascii(&args[1])?, state)?;
                            if index < 0 {
                                return Err(Error::new(pc, "字符串编号为负"));
                            }
                            let value = if head == b"strn" {
                                expression::eval_parameter(text::ascii(&args[2])?, state)?
                                    .to_string()
                                    .into_bytes()
                            } else {
                                args[2].clone()
                            };
                            state.strings.insert(index as u32, value);
                            continue;
                        }
                        b"wait"
                            if args.len() == 2
                                && args[1].first().is_some_and(|b| {
                                    b.is_ascii_digit() || matches!(b, b'#' | b'(' | b'-' | b'@')
                                }) =>
                        {
                            Event::Wait(expression::eval_parameter(text::ascii(&args[1])?, state)?)
                        }
                        b"fselect" => {
                            if args.len() > 2 {
                                return Err(Error::new(pc, "fselect 的扩展参数尚未实现"));
                            }
                            let interface = args.get(1).map(|s| text::cp932(s)).transpose()?;
                            let choices = collect(frame, state, &self.language)?;
                            if choices.is_empty() {
                                continue;
                            }
                            Event::Choices { interface, choices }
                        }
                        _ => Event::Command(args),
                    }
                }
                _ => {
                    return Err(Error::new(
                        pc,
                        format!("未支持的 CST 记录类型 {:02x}", record.kind),
                    ));
                }
            };
            self.pending = Some(event.clone());
            return Ok(Step::Event(event));
        }
        Ok(Step::Yield)
    }
}

fn command_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in bytes {
        if b == b'$' {
            out.extend(b"0x");
        } else {
            out.push(b);
        }
    }
    out
}

fn conditional<'a>(mut command: &'a [u8], state: &mut State) -> Result<Option<&'a [u8]>> {
    while let Some((condition, tail)) = text::condition(command)? {
        if expression::eval_parameter(text::ascii(condition)?, state)? == 0 {
            return Ok(None);
        }
        command = tail;
    }
    Ok(Some(command))
}

fn collect(frame: &mut Frame, state: &mut State, language: &str) -> Result<Vec<Choice>> {
    let mut choices = Vec::new();
    let mut scanned = 0;
    while let Some(record) = frame.scene.script.records.get(frame.pc) {
        scanned += 1;
        if scanned > 4096 {
            return Err(Error::new(frame.pc, "菜单记录数量超过上限"));
        }
        if record.kind != 0x30 {
            break;
        }
        let bytes = command_bytes(&text::expand(&record.bytes, &state.strings)?);
        let Some(command) = conditional(&bytes, state)? else {
            frame.pc += 1;
            continue;
        };
        let args = text::arguments(command)?;
        if args.len() < 3 {
            break;
        }
        let id = expression::eval_parameter(text::ascii(&args[0])?, state)?;
        let translated = frame
            .scene
            .localization
            .as_ref()
            .and_then(|t| t.option(language, frame.scene.script.inputs.len(), choices.len()))
            .filter(|s| !s.is_empty());
        choices.push(Choice {
            id,
            target: text::cp932(&args[1])?,
            text: translated.unwrap_or(text::cp932(&args[2])?),
        });
        frame.pc += 1;
    }
    Ok(choices)
}
