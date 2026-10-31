use super::{Interpreter, Line};
use crate::{Error, Result, expression::State, text};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectReference {
    pub name: String,
    /// −1 表示整个数组，其余值为银行编号。
    pub bank: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectAttributes {
    pub file: Option<Vec<u8>>,
    pub image_number: Option<i32>,
    pub frames: [Option<i32>; 32],
    pub position: [Option<i32>; 3],
    pub plane: Option<ObjectReference>,
    pub blend: Option<i32>,
    pub parent: Option<ObjectReference>,
    pub base: Option<[i32; 3]>,
    pub size: Option<[i32; 2]>,
    pub vram: Option<[i32; 2]>,
    pub priority: Option<i32>,
    pub sequential_priority: bool,
    pub parent_priority: Option<i32>,
    pub display: Option<i32>,
    pub enabled: Option<i32>,
    pub default_events: Option<Vec<u8>>,
    pub color: Option<i32>,
    pub sound_type: Option<i32>,
    pub channel: Option<i32>,
    /// 顺序为上、下、左、右；空名称表示解除绑定。
    pub keys: [Option<ObjectReference>; 4],
    pub position_resource: Option<Vec<u8>>,
    pub size_resource: Option<Vec<u8>>,
    pub object_type: Option<i32>,
    pub keyblock: Option<[i32; 3]>,
    pub table: Option<Vec<u8>>,
    pub group: Option<Vec<u8>>,
    pub system_key: Option<i32>,
    pub mask: Option<i32>,
    pub count: Option<i32>,
    pub button_type: Option<i32>,
    pub division: Option<i32>,
    pub extended_position: Option<i32>,
    pub path_mode: Option<i32>,
    pub viewport: Option<ObjectReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectInitialization {
    pub target: ObjectReference,
    pub attributes: ObjectAttributes,
}

pub(super) struct InitializationBlock {
    pub header: Vec<u8>,
    pub columns: Option<Vec<u32>>,
    pub lines: VecDeque<Line>,
}

const COLUMNS: &[(&[u8], u32)] = &[
    (b"FILE", 0),
    (b"IDN", 1),
    (b"ID", 2),
    (b"POS", 34),
    (b"PL", 35),
    (b"BLEND", 36),
    (b"PARENT", 37),
    (b"BASE", 38),
    (b"SIZE", 39),
    (b"VRAM", 40),
    (b"PRI", 41),
    (b"PRI_P", 42),
    (b"DISP", 43),
    (b"ENABLE", 44),
    (b"DEFAULT", 45),
    (b"COLOR", 46),
    (b"STYPE", 47),
    (b"CHANNEL", 48),
    (b"KEY_U", 49),
    (b"KEY_D", 50),
    (b"KEY_L", 51),
    (b"KEY_R", 52),
    (b"POS2", 53),
    (b"SIZE2", 54),
    (b"TYPE", 55),
    (b"KEYBLOCK", 56),
    (b"TABLE", 57),
    (b"GROUP", 58),
    (b"SYSKEY", 59),
    (b"MASK", 60),
    (b"COUNT", 61),
    (b"BTYPE", 62),
    (b"DIV", 63),
    (b"EXPOS", 64),
    (b"PATHMODE", 65),
    (b"VIEWPORT", 66),
];

impl Interpreter {
    pub(super) fn initialization_columns(&self, header: &[u8], state: &State) -> Result<Vec<u32>> {
        let expanded = text::expand(header, &state.strings)?;
        let args = self.parameters(&expanded, state)?;
        let mut columns = Vec::new();
        for arg in args.iter().skip(1) {
            let split = arg.iter().position(|&b| b == b'.').unwrap_or(arg.len());
            let base = COLUMNS
                .iter()
                .find(|(name, _)| *name == &arg[..split])
                .map(|&(_, code)| code)
                .ok_or_else(|| Error::new(0, "未知 FES 初始化列"))?;
            let suffix = arg.get(split + 1..).unwrap_or(&[]);
            let offset = if suffix.first().is_some_and(u8::is_ascii_digit) {
                suffix_number(suffix)
            } else {
                match suffix {
                    b"D" => 1,
                    b"F" => 2,
                    b"P" => 3,
                    _ => 0,
                }
            };
            let code = base.wrapping_add(offset);
            if code == u32::MAX || columns.len() == 67 {
                return Err(Error::new(0, "FES 初始化列超出上限"));
            }
            columns.push(code);
        }
        Ok(columns)
    }

    pub fn object_reference(&mut self, input: &[u8], state: &mut State) -> Result<ObjectReference> {
        let split = input
            .iter()
            .position(|&b| matches!(b, b'[' | b' '))
            .unwrap_or(input.len());
        let name = text::expand(&input[..split], &state.strings)?;
        let name = text::ascii(&name)?.to_owned();
        if name.is_empty() {
            return Err(Error::new(0, "FES 对象名称为空"));
        }
        let mut bank = if input.get(split) == Some(&b'[') {
            let start = split + 1;
            let end = input[start..]
                .iter()
                .position(|&b| matches!(b, b']' | b' '))
                .map(|i| start + i)
                .filter(|&end| end > start && input[end] == b']')
                .ok_or_else(|| Error::new(0, "FES 对象银行表达式无效"))?;
            self.eval(&input[start..end], state)?
        } else {
            -1
        };
        if bank == -1 && self.object_banks.get(&name).copied().unwrap_or(0) <= 1 {
            bank = 0;
        }
        Ok(ObjectReference { name, bank })
    }

    pub(super) fn initialize_object(
        &mut self,
        line: &Line,
        columns: &[u32],
        state: &mut State,
    ) -> Result<ObjectInitialization> {
        let expanded = text::expand(&line.bytes, &state.strings)?;
        let args = self.parameters(&expanded, state)?;
        let target = self.object_reference(
            args.first()
                .ok_or_else(|| Error::new(line.offset, "FES 初始化缺少对象"))?,
            state,
        )?;
        let mut a = ObjectAttributes::default();
        for (&column, arg) in columns.iter().zip(args.iter().skip(1)) {
            let result = (|| {
                match column {
                    0 => a.file = Some(arg.clone()),
                    1 => a.image_number = self.component(arg, 0, state)?,
                    2..=33 => a.frames[(column - 2) as usize] = self.component(arg, 0, state)?,
                    34 => {
                        for (i, part) in a.position.iter_mut().enumerate() {
                            if let Some(value) = self.component(arg, i, state)? {
                                *part = Some(value);
                            }
                        }
                    }
                    35 => a.plane = Some(self.object_reference(arg, state)?),
                    36 => a.blend = self.component(arg, 0, state)?,
                    37 => a.parent = Some(self.object_reference(arg, state)?),
                    38 => {
                        let parts = a.base.get_or_insert([0; 3]);
                        self.components(arg, parts, state)?;
                    }
                    39 => {
                        let parts = a.size.get_or_insert([0; 2]);
                        self.components(arg, parts, state)?;
                    }
                    40 => {
                        let parts = a.vram.get_or_insert([0; 2]);
                        self.components(arg, parts, state)?;
                    }
                    41 => {
                        a.priority = self.component(arg, 0, state)?;
                        a.sequential_priority = target.bank == -1;
                    }
                    42 => a.parent_priority = self.component(arg, 0, state)?,
                    43 => a.display = self.component(arg, 0, state)?,
                    44 => a.enabled = self.component(arg, 0, state)?,
                    45 => a.default_events = Some(arg.clone()),
                    46 => a.color = self.component(arg, 0, state)?,
                    47 => a.sound_type = self.component(arg, 0, state)?,
                    48 => a.channel = self.component(arg, 0, state)?,
                    49..=52 => {
                        let mut reference = self.object_reference(arg, state)?;
                        if reference.name == "0" {
                            reference.name.clear();
                            reference.bank = 0;
                        }
                        a.keys[(column - 49) as usize] = Some(reference);
                    }
                    53 => {
                        a.position_resource =
                            Some(self.object_reference(arg, state)?.name.into_bytes())
                    }
                    54 => {
                        a.size_resource = Some(self.object_reference(arg, state)?.name.into_bytes())
                    }
                    55 => a.object_type = self.component(arg, 0, state)?,
                    56 => {
                        let parts = a.keyblock.get_or_insert([0; 3]);
                        self.components(arg, parts, state)?;
                    }
                    57 => a.table = Some(arg.clone()),
                    58 => a.group = Some(arg.clone()),
                    59 => a.system_key = self.component(arg, 0, state)?,
                    60 => a.mask = self.component(arg, 0, state)?,
                    61 => a.count = self.component(arg, 0, state)?,
                    62 => a.button_type = self.component(arg, 0, state)?,
                    63 => a.division = self.component(arg, 0, state)?,
                    64 => a.extended_position = self.component(arg, 0, state)?,
                    65 => a.path_mode = self.component(arg, 0, state)?,
                    66 => a.viewport = Some(self.object_reference(arg, state)?),
                    _ => (),
                }
                Ok::<_, Error>(())
            })();
            result.map_err(|error| {
                Error::new(
                    error.offset,
                    format!("FES 初始化列 {column}: {}", error.message),
                )
            })?;
        }
        Ok(ObjectInitialization {
            target,
            attributes: a,
        })
    }

    pub(super) fn component(
        &mut self,
        input: &[u8],
        index: usize,
        state: &mut State,
    ) -> Result<Option<i32>> {
        input
            .split(|&b| b == b',')
            .nth(index)
            .map(|part| {
                if part.is_empty() {
                    Ok(0)
                } else {
                    self.eval(part, state)
                }
            })
            .transpose()
    }

    fn components(&mut self, input: &[u8], output: &mut [i32], state: &mut State) -> Result<()> {
        for (i, value) in output.iter_mut().enumerate() {
            let Some(part) = self.component(input, i, state)? else {
                break;
            };
            *value = part;
        }
        Ok(())
    }
}

fn suffix_number(input: &[u8]) -> u32 {
    let (input, radix) = if input.starts_with(b"0x") || input.starts_with(b"0X") {
        (&input[2..], 16)
    } else {
        (input, 10)
    };
    input
        .iter()
        .map_while(|&b| (b as char).to_digit(radix))
        .fold(0u32, |n, d| n.wrapping_mul(radix).wrapping_add(d))
}
