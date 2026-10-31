use super::{Interpreter, resources::integer_prefix};
use crate::{Error, Result, expression::State};
use std::collections::BTreeMap;

const DESCRIPTORS: &[&str] = &[
    "p", "BX", "bx", "BY", "by", "BZ", "bz", "PX", "px", "PY", "py", "PZ", "pz", "VX", "vx", "VY",
    "vy", "VZ", "vz", "RX", "rx", "RY", "ry", "RZ", "rz", "SX", "sx", "SY", "sy", "SZ", "sz", "CR",
    "cr", "CG", "cg", "CB", "cb", "CA", "ca", "CX", "cx",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubcommandMode {
    Oneshot,
    Continue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubcommandParameter {
    pub descriptor: &'static str,
    pub default: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappedArgument {
    Parameter(i32),
    Integer(i32),
    Context(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subcommand {
    pub mode: SubcommandMode,
    pub name: Vec<u8>,
    pub target: Vec<u8>,
    pub parameters: Vec<SubcommandParameter>,
    pub arguments: Vec<MappedArgument>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubcommandTable {
    pub bindings: BTreeMap<Vec<u8>, Subcommand>,
    pub diagnostics: Vec<Error>,
}

/// 由宿主按本次目标银行提供的对象状态，旋转使用弧度。
#[derive(Clone, Debug, PartialEq)]
pub struct SubcommandContext {
    pub base: [f32; 3],
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub color: u32,
}

impl Default for SubcommandContext {
    fn default() -> Self {
        Self {
            base: [0.0; 3],
            position: [0.0; 3],
            velocity: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            color: 0,
        }
    }
}

impl Interpreter {
    /// arguments 为去除对象名和命令名、已展开字符串的参数词项。
    /// 每次 Parameter 映射顺序消耗一个输入，编号选择描述符和缺省值。
    pub fn subcommand_arguments(
        &mut self,
        binding: &Subcommand,
        arguments: &[Vec<u8>],
        context: &SubcommandContext,
        state: &mut State,
    ) -> Result<Vec<f32>> {
        if binding.arguments.len() > 16 || binding.parameters.len() > 16 {
            return Err(Error::new(0, "SUBCOMMAND 参数超过上限"));
        }
        let mut input = arguments.iter();
        let mut output = Vec::with_capacity(binding.arguments.len());
        for (position, mapped) in binding.arguments.iter().enumerate() {
            output.push(match mapped {
                MappedArgument::Integer(n) => *n as f32,
                MappedArgument::Context(name) => color_component(context.color, name)
                    .ok_or_else(|| Error::new(position, "未知 SUBCOMMAND 上下文标记"))?,
                MappedArgument::Parameter(index) => {
                    let parameter = usize::try_from(*index)
                        .ok()
                        .and_then(|i| binding.parameters.get(i))
                        .ok_or_else(|| Error::new(position, "SUBCOMMAND 映射编号越界"))?;
                    let bytes = input.next().map_or(&parameter.default[..], Vec::as_slice);
                    if matches!(parameter.descriptor, "CX" | "cx") {
                        // 基线保留描述符，但调用分派不对这两项求值。
                        0.0
                    } else {
                        let bytes = if bytes.is_empty() {
                            binding
                                .parameters
                                .get(position)
                                .map_or(&[][..], |p| &p.default)
                        } else {
                            bytes
                        };
                        let (current, base, relative, angle) =
                            descriptor_context(parameter.descriptor, context)?;
                        let percent = bytes.ends_with(b"%");
                        let bytes = if percent {
                            &bytes[..bytes.len() - 1]
                        } else {
                            bytes
                        };
                        let value = self.eval_with_current(bytes, state, integer_float(current))?;
                        let mut value = value as f32;
                        if angle {
                            value = value * std::f32::consts::PI / 180.0;
                        }
                        if percent {
                            value = value * base / 100.0;
                        }
                        if relative {
                            value -= current;
                        }
                        value
                    }
                }
            });
        }
        Ok(output)
    }

    /// 读取运动子命令声明；默认值留待调用时按描述符解释。
    pub fn subcommands(&self, state: &State) -> SubcommandTable {
        let mut table = SubcommandTable::default();
        for declaration in self
            .program
            .declarations
            .iter()
            .filter(|d| d.header == b"SUBCOMMAND")
        {
            for line in &declaration.lines {
                match self.subcommand(&line.bytes, state) {
                    Ok(binding) => {
                        table
                            .bindings
                            .entry(binding.name.clone())
                            .or_insert(binding);
                    }
                    Err(error) => table
                        .diagnostics
                        .push(Error::new(line.offset + error.offset, error.message)),
                }
            }
        }
        table
    }

    fn subcommand(&self, input: &[u8], state: &State) -> Result<Subcommand> {
        let args = self.parameters(input, state)?;
        let mode = match args.first().map(Vec::as_slice) {
            Some(b"oneshot") => SubcommandMode::Oneshot,
            Some(b"continue") => SubcommandMode::Continue,
            _ => return Err(Error::new(0, "未知 SUBCOMMAND 类别")),
        };
        let name = args
            .get(1)
            .ok_or_else(|| Error::new(0, "SUBCOMMAND 缺少命令名"))?
            .clone();
        let target = args
            .get(2)
            .ok_or_else(|| Error::new(0, "SUBCOMMAND 缺少运动资源名"))?
            .clone();
        let colon = args
            .iter()
            .enumerate()
            .skip(3)
            .find(|(_, arg)| *arg == b":")
            .map(|(i, _)| i)
            .ok_or_else(|| Error::new(0, "SUBCOMMAND 缺少映射分隔符"))?;
        if colon - 3 > 16 || args.len() - colon - 1 > 16 {
            return Err(Error::new(0, "SUBCOMMAND 参数超过上限"));
        }
        let mut parameters = Vec::new();
        for input in &args[3..colon] {
            let (name, default) = name_and_value(input)?;
            let descriptor = DESCRIPTORS
                .iter()
                .find(|d| d.as_bytes() == name)
                .copied()
                .ok_or_else(|| Error::new(0, "未知 SUBCOMMAND 参数描述符"))?;
            if default.len() > 15 {
                return Err(Error::new(0, "SUBCOMMAND 默认值过长"));
            }
            parameters.push(SubcommandParameter {
                descriptor,
                default: default.to_vec(),
            });
        }
        let mut arguments = Vec::new();
        for input in &args[colon + 1..] {
            let (name, value) = name_and_value(input)?;
            arguments.push(if name.is_empty() {
                MappedArgument::Parameter(
                    integer_prefix(value)
                        .ok_or_else(|| Error::new(0, "SUBCOMMAND 映射编号无效"))?,
                )
            } else if let Some(context) = ["cxr", "cxg", "cxb", "cxa", "CA"]
                .into_iter()
                .find(|s| s.as_bytes() == name)
            {
                MappedArgument::Context(context)
            } else {
                MappedArgument::Integer(
                    integer_prefix(name).ok_or_else(|| Error::new(0, "SUBCOMMAND 映射整数无效"))?,
                )
            });
        }
        Ok(Subcommand {
            mode,
            name,
            target,
            parameters,
            arguments,
        })
    }
}

fn integer_float(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn color_component(color: u32, descriptor: &str) -> Option<f32> {
    let shift = match descriptor {
        "CR" | "cr" | "cxr" => 16,
        "CG" | "cg" | "cxg" => 8,
        "CB" | "cb" | "cxb" => 0,
        "CA" | "ca" | "cxa" => 24,
        _ => return None,
    };
    Some(((color >> shift) & 255) as f32)
}

fn descriptor_context(
    descriptor: &str,
    context: &SubcommandContext,
) -> Result<(f32, f32, bool, bool)> {
    if descriptor == "p" {
        return Ok((0.0, 0.0, false, false));
    }
    if let Some(value) = color_component(context.color, descriptor) {
        return Ok((value, value, descriptor.as_bytes()[0] == b'c', false));
    }
    let bytes = descriptor.as_bytes();
    let (group, axis) = match bytes {
        [group, axis] => (group.to_ascii_uppercase(), axis.to_ascii_uppercase()),
        _ => return Err(Error::new(0, "未知 SUBCOMMAND 参数描述符")),
    };
    let axis = match axis {
        b'X' => 0,
        b'Y' => 1,
        b'Z' => 2,
        _ => return Err(Error::new(0, "未知 SUBCOMMAND 参数描述符")),
    };
    let values = match group {
        b'B' => context.base,
        b'P' => context.position,
        b'V' => context.velocity,
        b'R' => context.rotation,
        b'S' => context.scale,
        _ => return Err(Error::new(0, "未知 SUBCOMMAND 参数描述符")),
    };
    if !DESCRIPTORS.contains(&descriptor) {
        return Err(Error::new(0, "未知 SUBCOMMAND 参数描述符"));
    }
    let current = values[axis];
    Ok((
        current,
        if group == b'S' { 1.0 } else { current },
        bytes[0].is_ascii_lowercase(),
        group == b'R',
    ))
}

fn name_and_value(input: &[u8]) -> Result<(&[u8], &[u8])> {
    let split = input
        .iter()
        .position(|&b| matches!(b, b'[' | b' '))
        .unwrap_or(input.len());
    if input.get(split) != Some(&b'[') {
        return Ok((&input[..split], b"-1"));
    }
    let start = split + 1;
    let end = input[start..]
        .iter()
        .position(|&b| matches!(b, b']' | b' '))
        .map(|i| start + i)
        .filter(|&i| i > start && input[i] == b']')
        .ok_or_else(|| Error::new(split, "SUBCOMMAND 方括号值无效"))?;
    Ok((&input[..split], &input[start..end]))
}
