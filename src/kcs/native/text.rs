use super::{words, write_string};
use crate::kcs::{HostState, Interpreter, NativeReturn, numeric, strings};
use crate::{Error, Result, expression, text};

pub(super) fn call(
    state: &mut expression::State,
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let count = match index {
        17 | 18 | 25 | 26 | 40 | 47 | 48 | 278 => 1,
        13..=16 | 19..=24 | 46 | 276 | 277 | 540 | 598 | 1002 | 1045 | 1046 | 1051 | 1063 => 2,
        37 | 41..=43 | 519 | 539 => 3,
        38 | 44 | 45 => 4,
        _ => unreachable!(),
    };
    let args = words(arguments, count, vm.pc)?;
    Ok(match index {
        37 => {
            let value = args[1] as i32;
            let mut digits = Vec::new();
            // MIN 的绝对值仍为负数，负余数直接参与数字字节计算。
            let mut n = value.wrapping_abs();
            if n == 0 {
                digits.push(b'0');
            } else {
                let mut group = 0;
                while n != 0 {
                    if group == 3 {
                        digits.push(b',');
                        group = 0;
                    }
                    digits.push(((n % 10) as u8).wrapping_add(b'0'));
                    n /= 10;
                    group += 1;
                }
                if value < 0 {
                    digits.push(b'-');
                } else if args[2] != 0 {
                    digits.push(b'+');
                }
            }
            digits.reverse();
            write_string(vm, args[0], &digits, 2)?;
            NativeReturn::Continue
        }
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
            text::integer_prefix(&strings::format(vm, args[0], 0)?).map_or(0, |(_, value)| value),
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
            drop(source);
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
            drop(source);
            vm.write(args[0].wrapping_add(length as u32), &[0, 0])?;
            NativeReturn::Value(length as u32)
        }
        40 => {
            NativeReturn::Value((strings::character_width(&vm.read_string(args[0])?) == 2) as u32)
        }
        46 => {
            if vm.read(args[0], 1).is_err() {
                return Ok(NativeReturn::Continue);
            }
            let source = vm.read_string(args[0])?;
            let percentage = source
                .ends_with(b"%")
                .then(|| text::integer_prefix(&source))
                .flatten();
            drop(source);
            if let Some((_, value)) = percentage {
                let value = numeric::convert(0x37, value);
                let base = numeric::convert(0x36, args[1]);
                let product = numeric::float_ieee(3, value, base);
                let scaled = numeric::float_ieee(4, product, 100f32.to_bits());
                let result = numeric::convert(0x39, scaled) as i32;
                write_string(vm, args[0], result.to_string().as_bytes(), 1)?;
            }
            NativeReturn::Continue
        }
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
            drop(source);
            drop(separators);
            write_string(vm, args[0], &value, 2)?;
            NativeReturn::Value(length as u32)
        }
        41 => {
            let value = strings::split(&vm.read_string(args[1])?, &vm.read_string(args[2])?);
            vm.write(args[0], &value)?;
            NativeReturn::Continue
        }
        42 => {
            let value = strings::multi(vm, args[1], args[2] as i32)?.to_vec();
            write_string(vm, args[0], &value, 2)?;
            NativeReturn::Value((!value.is_empty()) as u32)
        }
        43 => {
            if args.iter().all(|&address| vm.read(address, 1).is_ok()) {
                strings::split_parenthesized(vm, args[0], args[1], args[2])?;
            }
            NativeReturn::Continue
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
        539 => NativeReturn::Value(strings::integer_sequence(
            vm,
            state,
            args[0],
            args[1],
            args[2] as i32,
        )?),
        540 => NativeReturn::Value(strings::word_offset(vm, args[0], args[1])?),
        598 => {
            if let Some(name) = strings::comma_name(vm, args[0])? {
                write_string(vm, args[1], &name, 1)?;
                NativeReturn::Value(1)
            } else {
                NativeReturn::Value(0)
            }
        }
        // 状态已经由调用者初始化，原入口在已有数值区时同样保留它。
        276 => {
            state
                .strings
                .insert(args[0], vm.read_string(args[1])?.to_vec());
            NativeReturn::Continue
        }
        277 => {
            let string = state.strings.get(&args[0]);
            write_string(vm, args[1], string.map_or(&[], Vec::as_slice), 1)?;
            NativeReturn::Value(string.is_some() as u32)
        }
        278 => {
            let string = text::expand(&vm.read_string(args[0])?, &state.strings)?;
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
            let (output, terminators) = strings::replace(&source, &old, &replacement)
                .map_err(|error| Error::new(vm.pc, error.message))?;
            drop(source);
            write_string(vm, args[0], &output, terminators)?;
            NativeReturn::Continue
        }
        1002 => {
            let mut value = crate::kcs::format_string(vm, args[1], 0)?;
            value.truncate(
                value
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(value.len()),
            );
            write_string(vm, args[0], &value, 1)?;
            for (find, replacement) in &host.script_replacements {
                let find = &find[..find
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(find.len())];
                let replacement = &replacement[..replacement
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(replacement.len())];
                let (output, terminators) = strings::replace(&value, find, replacement)
                    .map_err(|error| Error::new(vm.pc, error.message))?;
                value = output;
                write_string(vm, args[0], &value, terminators)?;
            }
            NativeReturn::Continue
        }
        1045 | 1046 | 1051 | 1063 => crate::kcs::utf8::call(index, &args, vm)?,
        519 => {
            vm.write(args[0], &[0])?;
            let words = text::arguments(&vm.read_string(args[1])?)?;
            let word = words.get(args[2] as usize);
            if let Some(word) = word {
                write_string(vm, args[0], word, 2)?;
            }
            NativeReturn::Value(word.is_some() as u32)
        }
        _ => unreachable!(),
    })
}
