use super::{Host, Interpreter, NativeReturn, TaskClock};
use super::{numeric, strings};
use crate::{Error, Result, expression, read, text};

/// 在已有数值状态上提供普通任务表中已核实的接口。
pub struct CoreHost<'a> {
    pub state: &'a mut expression::State,
    pub configured_shared_slots: u32,
    pub debug_messages: Vec<Vec<u8>>,
    pub task_time: u32,
    pub task_origin: u32,
}

impl<'a> CoreHost<'a> {
    pub fn new(state: &'a mut expression::State) -> Self {
        Self {
            configured_shared_slots: state.shared.len() as u32,
            debug_messages: Vec::new(),
            task_time: 0,
            task_origin: 0,
            state,
        }
    }

    pub fn set_task_clock(&mut self, clock: &TaskClock) {
        self.task_origin = clock.origin;
        self.task_time = clock.time;
    }
}

impl Host for CoreHost<'_> {
    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if matches!(index, 0 | 2 | 3) {
            return Ok(NativeReturn::Continue);
        }
        if index == 65 {
            return Ok(NativeReturn::YieldVoid);
        }
        let count = match index {
            1 | 6 | 17 | 18 | 25 | 26 | 40 | 47 | 48 | 71 | 209 | 210 | 212 | 213 | 278 | 319 => 1,
            13..=16 | 19..=24 | 211 | 276 | 277 => 2,
            50..=58 | 60 | 62..=64 => 1,
            59 | 61 => 2,
            38 | 44 | 45 => 4,
            41 | 42 | 519 => 3,
            5 | 789 => 0,
            _ => return Err(Error::new(vm.pc, format!("未支持的 KCS native {index}"))),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        Ok(match index {
            1 => {
                self.debug_messages.push(strings::format(vm, args[0], 0)?);
                NativeReturn::Continue
            }
            6 => NativeReturn::Value(args[0]),
            5 => NativeReturn::Value(self.task_time.wrapping_sub(self.task_origin)),
            71 => {
                vm.wait_for_message(args[0])?;
                NativeReturn::YieldVoid
            }
            319 => {
                let shifted = numeric::float_ieee(1, args[0], 0.5f32.to_bits());
                let rounded = numeric::native_math(15, &[shifted]);
                NativeReturn::Value(numeric::convert(0x39, rounded))
            }
            50..=61 => {
                let function = match index {
                    50..=55 => index - 44,
                    56..=61 => index - 43,
                    _ => unreachable!(),
                };
                let mut value = numeric::native_math(function, &args);
                // 普通表调用 SSE2 precise pow，奇整数指数会清除 NaN 底数符号。
                let exponent = args.get(1).copied().map(f32::from_bits).unwrap_or(0.0);
                if index == 61
                    && f32::from_bits(args[0]).is_nan()
                    && exponent.abs() < 16_777_216.0
                    && exponent.fract() == 0.0
                    && exponent as i32 & 1 != 0
                {
                    value &= 0x7fff_ffff;
                }
                NativeReturn::Value(value)
            }
            62 | 63 => {
                let (multiplier, divisor) = if index == 62 {
                    (std::f32::consts::PI.to_bits(), 180.0f32.to_bits())
                } else {
                    (180.0f32.to_bits(), std::f32::consts::PI.to_bits())
                };
                let product = numeric::float_ieee(3, args[0], multiplier);
                NativeReturn::Value(numeric::float_ieee(4, product, divisor))
            }
            64 => NativeReturn::Value((args[0] as i32).wrapping_abs() as u32),
            13 => {
                let source = vm.read_string(args[1])?.to_vec();
                if args[0] > args[1] && args[0] - args[1] <= source.len() as u32 {
                    return Err(Error::new(vm.pc, "KCS 字符串复制会覆盖尚未读取的源"));
                }
                write_string(vm, args[0], &source, 2)?;
                NativeReturn::Value(args[0])
            }
            14 => {
                let text = strings::format(vm, args[1], 0)?;
                write_string(vm, args[0], &text, 2)?;
                NativeReturn::Value(args[0])
            }
            15 => {
                strings::append_raw(vm, args[0], args[1])?;
                NativeReturn::Value(args[0])
            }
            16 => {
                let source = strings::format(vm, args[1], 0)?;
                let length = vm.read_string(args[0])?.len();
                write_string(vm, args[0].wrapping_add(length as u32), &source, 2)?;
                NativeReturn::Value(args[0])
            }
            17 => NativeReturn::Value(vm.read_string(args[0])?.len() as u32),
            18 => NativeReturn::Value(strings::format(vm, args[0], 0)?.len() as u32),
            19..=22 => {
                let (left, right) = if index.is_multiple_of(2) {
                    (
                        strings::format(vm, args[0], 0)?,
                        strings::format(vm, args[1], 0)?,
                    )
                } else {
                    (
                        vm.read_string(args[1])?.to_vec(),
                        vm.read_string(args[0])?.to_vec(),
                    )
                };
                NativeReturn::Value(strings::compare(&left, &right, index >= 21) as u32)
            }
            26 => NativeReturn::Value(strings::float32(&strings::format(vm, args[0], 0)?)),
            25 => NativeReturn::Value(
                text::integer_prefix(&strings::format(vm, args[0], 0)?)
                    .map_or(0, |(_, value)| value),
            ),
            23 => {
                let source = vm.read_string(args[0])?;
                let mut length = 0;
                while length < source.len() {
                    let width = strings::character_width(&source[length..]);
                    if length + width >= args[1] as usize || length + width > source.len() {
                        break;
                    }
                    length += width;
                }
                vm.write(args[0].wrapping_add(length as u32), &[0, 0])?;
                NativeReturn::Value(length as u32)
            }
            24 => {
                let source = vm.read_string(args[0])?;
                let mut length = 0;
                for _ in 0..args[1] {
                    if length == source.len() {
                        break;
                    }
                    length += strings::character_width(&source[length..]);
                }
                vm.write(args[0].wrapping_add(length as u32), &[0, 0])?;
                NativeReturn::Value(length as u32)
            }
            40 => NativeReturn::Value(
                (strings::character_width(vm.read_string(args[0])?) == 2) as u32,
            ),
            47 | 48 => {
                let source = if index == 48 {
                    strings::format(vm, args[0], 0)?
                } else {
                    vm.read_string(args[0])?.to_vec()
                };
                let mut length = 0;
                let mut count = 0;
                while length < source.len() {
                    length += strings::character_width(&source[length..]);
                    count += 1;
                }
                NativeReturn::Value(count)
            }
            38 => {
                let separators = vm.read_string(args[2])?;
                let mut decoded = Vec::new();
                let mut p = 0;
                while p < separators.len() {
                    if separators[p..].starts_with(b"\\t") {
                        decoded.push(b'\t');
                        p += 2;
                    } else {
                        decoded.push(separators[p]);
                        p += 1;
                    }
                }
                let source = vm.read_string(args[1])?;
                let length = source
                    .iter()
                    .position(|b| decoded.contains(b))
                    .unwrap_or(source.len())
                    .min(args[3].wrapping_sub(2) as usize);
                let value = source[..length].to_vec();
                write_string(vm, args[0], &value, 2)?;
                NativeReturn::Value(length as u32)
            }
            41 => {
                let value = strings::split(vm.read_string(args[1])?, vm.read_string(args[2])?);
                vm.write(args[0], &value)?;
                NativeReturn::Continue
            }
            42 => {
                let value = strings::multi(vm, args[1], args[2] as i32)?.to_vec();
                write_string(vm, args[0], &value, 2)?;
                NativeReturn::Value((!value.is_empty()) as u32)
            }
            44 => {
                vm.write(args[0], &[0])?;
                let first = (args[2] as i32).max(0);
                let last = if args[3] == u32::MAX {
                    i32::MAX
                } else {
                    args[3] as i32
                };
                let value = strings::join(vm, args[1], first, last)?;
                write_string(vm, args[0], &value, 1)?;
                NativeReturn::Continue
            }
            // 状态已经由调用者初始化，原入口在已有数值区时同样保留它。
            209 => NativeReturn::Continue,
            210 => NativeReturn::Value(
                self.state
                    .shared
                    .get(args[0] as usize)
                    .copied()
                    .unwrap_or(0) as u32,
            ),
            211 => {
                if let Some(slot) = self.state.shared.get_mut(args[0] as usize) {
                    *slot = args[1] as i32;
                }
                NativeReturn::Continue
            }
            212 => {
                self.state.current = args[0] as i32;
                NativeReturn::Continue
            }
            213 => {
                let source = text::ascii(vm.read_string(args[0])?)?;
                NativeReturn::Value(expression::eval_parameter(source, self.state)? as u32)
            }
            276 => {
                self.state
                    .strings
                    .insert(args[0], vm.read_string(args[1])?.to_vec());
                NativeReturn::Continue
            }
            277 => {
                let string = self.state.strings.get(&args[0]);
                write_string(vm, args[1], string.map_or(&[], Vec::as_slice), 1)?;
                NativeReturn::Value(string.is_some() as u32)
            }
            278 => {
                let string = text::expand(vm.read_string(args[0])?, &self.state.strings)?;
                write_string(vm, args[0], &string, 2)?;
                NativeReturn::Continue
            }
            45 => {
                let old = vm.read_string(args[2])?.to_vec();
                let replacement = vm.read_string(args[3])?.to_vec();
                if old.is_empty() {
                    return Err(Error::new(vm.pc, "KCS 字符串替换的查找串为空"));
                }
                vm.write(args[0], &[0])?;
                let source = vm.read_string(args[1])?;
                let mut output = Vec::new();
                let mut p = 0;
                while p < source.len() {
                    if source[p..].starts_with(&old) {
                        output.extend(&replacement);
                        p += old.len();
                    } else {
                        output.push(source[p]);
                        p += 1;
                    }
                    if output.len() > read::MAX_BYTES {
                        return Err(Error::new(vm.pc, "KCS 字符串替换超过上限"));
                    }
                }
                write_string(vm, args[0], &output, 1)?;
                NativeReturn::Continue
            }
            519 => {
                vm.write(args[0], &[0])?;
                let words = text::arguments(vm.read_string(args[1])?)?;
                let word = words.get(args[2] as usize);
                if let Some(word) = word {
                    write_string(vm, args[0], word, 2)?;
                }
                NativeReturn::Value(word.is_some() as u32)
            }
            789 => NativeReturn::Value(self.configured_shared_slots),
            _ => unreachable!(),
        })
    }
}

fn write_string(
    vm: &mut Interpreter,
    address: u32,
    bytes: &[u8],
    terminators: usize,
) -> Result<()> {
    let mut output = bytes.to_vec();
    output.resize(output.len() + terminators, 0);
    vm.write(address, &output)
}
