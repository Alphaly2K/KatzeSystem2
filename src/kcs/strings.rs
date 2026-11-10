use super::{Interpreter, MemoryRead};
use crate::{Error, Result, expression, read, text};

pub(super) fn character_width(source: &[u8]) -> usize {
    match source {
        [0x81..=0x9f | 0xe0..=0xef, 0x40..=0x7e | 0x80..=0xfc, ..] => 2,
        _ => 1,
    }
}

pub(super) fn word_offset(vm: &Interpreter, address: u32, count: u32) -> Result<u32> {
    if count == 0 {
        return Ok(0);
    }
    let offset = (address & 0x7fff_ffff) as usize;
    let source = if address & 0x8000_0000 != 0 {
        vm.read(address, vm.external.len().saturating_sub(offset))?
    } else {
        let (storage, offset) = vm.local_region(offset)?;
        MemoryRead::Local(read::range(
            storage,
            offset,
            storage.len().saturating_sub(offset),
        )?)
    };
    let byte = |at: usize| {
        source
            .get(at)
            .copied()
            .ok_or_else(|| Error::new(vm.pc, "KCS 词项偏移读取越界"))
    };
    let mut at = 0;
    for _ in 0..count {
        while !matches!(byte(at)?, 0 | b' ') {
            at += 1;
        }
        if byte(at)? == 0 {
            return Ok(u32::MAX);
        }
        while byte(at)? == b' ' {
            at += 1;
        }
    }
    Ok(at as u32)
}

pub(super) fn comma_name(vm: &Interpreter, address: u32) -> Result<Option<Vec<u8>>> {
    let table = split(&format(vm, address, 0)?, b",");
    if table.len() > 256 {
        return Err(Error::new(vm.pc, "KCS 逗号词项表超过缓冲区上限"));
    }
    let mut parts = table
        .split(|&byte| byte == 0)
        .filter(|part| !part.is_empty());
    let Some(name) = parts.next() else {
        return Ok(None);
    };
    if parts.count() > 64 {
        return Err(Error::new(vm.pc, "KCS 逗号分量超过缓冲区上限"));
    }
    Ok(Some(name.to_vec()))
}

pub(super) fn integer_sequence(
    vm: &mut Interpreter,
    state: &mut expression::State,
    source: u32,
    destination: u32,
    maximum: i32,
) -> Result<u32> {
    if vm.read(source, 1).is_err() || vm.read(destination, 1).is_err() {
        return Ok(0);
    }
    if sequence_byte(vm, source)? != b'[' {
        if maximum <= 0 {
            return Ok(0);
        }
        let value = expression::eval_fes_fallback(text::ascii(&vm.read_string(source)?)?, state)?;
        vm.write(destination, &value.to_le_bytes())?;
        return Ok(1);
    }
    let mut scan = source;
    while !matches!(sequence_byte(vm, scan)?, 0 | b',') {
        scan += 1;
    }
    let mut cursor = source + 1;
    let mut written = 0;
    if sequence_byte(vm, scan)? == b',' {
        while (written as i32) < maximum && !matches!(sequence_byte(vm, cursor)?, 0 | b']') {
            let value = sequence_expression(vm, state, &mut cursor, b",]")?;
            vm.write(destination.wrapping_add(written * 4), &value.to_le_bytes())?;
            written += 1;
            // 输出可能覆盖分隔符，下一项的位置须在写入之后决定。
            if sequence_byte(vm, cursor)? == b',' {
                cursor += 1;
            }
        }
    } else {
        let mut first = sequence_expression(vm, state, &mut cursor, b"-]")?;
        if sequence_byte(vm, cursor)? == b'-' {
            cursor += 1;
        }
        let last = sequence_expression(vm, state, &mut cursor, b"]")?;
        while first <= last && (written as i32) < maximum {
            vm.write(destination.wrapping_add(written * 4), &first.to_le_bytes())?;
            written += 1;
            first = first.wrapping_add(1);
        }
    }
    Ok(written)
}

fn sequence_byte(vm: &Interpreter, address: u32) -> Result<u8> {
    Ok(vm.read(address, 1)?[0])
}

fn sequence_expression(
    vm: &Interpreter,
    state: &mut expression::State,
    cursor: &mut u32,
    separators: &[u8],
) -> Result<i32> {
    let mut source = Vec::new();
    loop {
        let byte = sequence_byte(vm, *cursor)?;
        if byte == 0 || separators.contains(&byte) {
            break;
        }
        if source.len() == 259 {
            return Err(Error::new(vm.pc, "KCS 整数序列表达式过长"));
        }
        source.push(byte);
        *cursor += 1;
    }
    expression::eval_fes_fallback(text::ascii(&source)?, state)
}

pub(super) use crate::text::float32;

pub(crate) fn format(vm: &Interpreter, address: u32, depth: usize) -> Result<Vec<u8>> {
    if depth >= 32 {
        return Err(Error::new(vm.pc, "KCS 格式字符串引用过深"));
    }
    let source = vm.read_string(address)?;
    let mut output = Vec::new();
    let mut p = 0;
    while p < source.len() {
        let b = source[p];
        if matches!(b, 0x81..=0x9f | 0xe0..=0xfc) && p + 1 < source.len() {
            output.extend(&source[p..p + 2]);
            p += 2;
        } else if source[p..].starts_with(b"%%") {
            output.push(b'%');
            p += 2;
        } else if b == b'%' {
            let token = source[p + 1..]
                .iter()
                .position(|&b| b == b'[')
                .map(|i| p + 1 + i);
            let parsed = token.and_then(|bracket| {
                let scope = *source.get(bracket + 1)?;
                if !matches!(scope, b'G' | b'L' | b'H') {
                    return None;
                }
                let mut end = bracket + 2;
                let mut offset = 0u32;
                while let Some(&b) = source.get(end).filter(|b| b.is_ascii_digit()) {
                    offset = offset.wrapping_mul(10).wrapping_add((b - b'0') as u32);
                    end += 1;
                }
                if source.get(end) != Some(&b']') {
                    return None;
                }
                Some((
                    bracket,
                    end + 1,
                    match scope {
                        b'L' => offset.wrapping_add(vm.frame_base as u32),
                        b'H' => offset | 0x8000_0000,
                        _ => offset,
                    },
                ))
            });
            if let Some((bracket, end, value_address)) = parsed {
                let spec = text::ascii(&source[p + 1..bracket])?;
                let conversion = spec
                    .as_bytes()
                    .last()
                    .copied()
                    .ok_or_else(|| Error::new(vm.pc, "KCS 格式转换为空"))?;
                let text = if conversion == b's' {
                    format(vm, value_address, depth + 1)?
                } else {
                    let n = read::u32_at(&vm.read(value_address, 4)?, 0)?;
                    match conversion {
                        b'd' | b'i' => (n as i32).to_string().into_bytes(),
                        b'u' => n.to_string().into_bytes(),
                        b'x' => format!("{n:x}").into_bytes(),
                        b'X' => format!("{n:X}").into_bytes(),
                        b'c' => vec![n as u8],
                        b'f' => {
                            let precision = spec[..spec.len() - 1]
                                .split_once('.')
                                .map(|(_, p)| field(p))
                                .transpose()?
                                .unwrap_or(6);
                            text::format_float(f32::from_bits(n), precision)
                        }
                        _ => return Err(Error::new(vm.pc, "未支持的 KCS 格式转换")),
                    }
                };
                output.extend(pad(spec, text)?);
                p = end;
            } else {
                output.push(b);
                p += 1;
            }
        } else {
            output.push(b);
            p += 1;
        }
        if output.len() > read::MAX_BYTES {
            return Err(Error::new(vm.pc, "KCS 格式输出超过上限"));
        }
    }
    if let Some(end) = output.iter().position(|&b| b == 0) {
        output.truncate(end);
    }
    Ok(output)
}

pub(super) fn compare(left: &[u8], right: &[u8], insensitive: bool) -> i32 {
    for i in 0..=left.len().min(right.len()) {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        if a != b {
            let difference = if insensitive {
                a.to_ascii_lowercase() as i32 - b.to_ascii_lowercase() as i32
            } else {
                a as i8 as i32 - b as i8 as i32
            };
            if difference != 0 {
                return difference;
            }
        }
    }
    0
}

pub(super) fn append_raw(vm: &mut Interpreter, target: u32, source: u32) -> Result<()> {
    let length = vm.read_string(source)?.len() + 1;
    let end = target.wrapping_add(vm.read_string(target)?.len() as u32);
    // 原入口先按 DWORD、再按单字节向前复制，重叠区不能先整体快照。
    let mut offset = 0;
    while offset < length {
        let width = if length - offset >= 4 { 4 } else { 1 };
        let mut bytes = [0; 4];
        bytes[..width].copy_from_slice(&vm.read(source.wrapping_add(offset as u32), width)?);
        vm.write(end.wrapping_add(offset as u32), &bytes[..width])?;
        offset += width;
    }
    let end = target.wrapping_add(vm.read_string(target)?.len() as u32);
    vm.write(end, &[0, 0])
}

fn field(s: &str) -> Result<usize> {
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<usize>()
        .ok()
        .filter(|&n| n <= 65536)
        .ok_or_else(|| Error::new(0, "KCS 格式宽度或精度无效"))
}

fn pad(spec: &str, mut value: Vec<u8>) -> Result<Vec<u8>> {
    let conversion = spec.as_bytes()[spec.len() - 1];
    let prefix = &spec[..spec.len() - 1];
    let (width, precision) = prefix
        .split_once('.')
        .map_or((prefix, None), |(a, b)| (a, Some(b)));
    let width = field(width)?;
    if conversion == b's' {
        if let Some(p) = precision {
            value.truncate(field(p)?);
        }
    } else if conversion != b'f' && precision.is_some() {
        return Err(Error::new(0, "KCS 整数格式精度尚未实现"));
    }
    if value.len() >= width {
        return Ok(value);
    }
    let zero = prefix.starts_with('0') && !matches!(conversion, b's' | b'c');
    let mut out = vec![if zero { b'0' } else { b' ' }; width - value.len()];
    if zero && value.first() == Some(&b'-') {
        value.remove(0);
        out.insert(0, b'-');
    }
    out.extend(value);
    Ok(out)
}

pub(super) fn multi(
    vm: &Interpreter,
    mut address: u32,
    index: i32,
) -> Result<super::MemoryRead<'_>> {
    for _ in 0..index {
        let word = vm.read_string(address)?;
        if word.is_empty() {
            return Ok(word);
        }
        address = address.wrapping_add(word.len() as u32 + 1);
    }
    vm.read_string(address)
}

pub(super) fn join(vm: &Interpreter, mut address: u32, first: i32, last: i32) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for index in 0..=last {
        let word = vm.read_string(address)?;
        if word.is_empty() {
            break;
        }
        if index >= first {
            output.extend_from_slice(&word);
            output.push(b' ');
            if output.len() > read::MAX_BYTES {
                return Err(Error::new(vm.pc, "KCS 词项拼接超过上限"));
            }
        }
        address = address.wrapping_add(word.len() as u32 + 1);
    }
    Ok(output)
}

pub(super) fn split(source: &[u8], separators: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut p = 0;
    while p < source.len() {
        let b = source[p];
        if separators.contains(&b) {
            if output.last().is_some_and(|&b| b != 0) {
                output.push(0);
            }
            p += 1;
        } else {
            let width = character_width(&source[p..]);
            output.extend(&source[p..p + width]);
            p += width;
        }
    }
    output.extend([0, 0]);
    output
}

pub(super) fn replace(source: &[u8], find: &[u8], replacement: &[u8]) -> Result<(Vec<u8>, usize)> {
    if source.is_empty() {
        return Ok((Vec::new(), 2));
    }
    if find.is_empty() {
        return Err(Error::new(0, "KCS 字符串替换的查找串为空"));
    }
    let mut output = Vec::new();
    let mut position = 0;
    let mut terminators = 1;
    while position < source.len() {
        if source[position..].starts_with(find) {
            output.extend_from_slice(replacement);
            position += find.len();
            if position == source.len() {
                terminators = 2;
            }
        } else {
            output.push(source[position]);
            position += 1;
        }
        if output.len() > read::MAX_BYTES {
            return Err(Error::new(0, "KCS 字符串替换超过上限"));
        }
    }
    Ok((output, terminators))
}

fn byte(vm: &Interpreter, address: u32) -> Result<u8> {
    Ok(vm.read(address, 1)?[0])
}

fn copy_parentheses(vm: &mut Interpreter, destination: u32, source: u32) -> Result<u32> {
    if byte(vm, source)? != b'(' {
        return Ok(0);
    }
    vm.write(destination, b"(")?;
    let mut frames = vec![(destination, source, 1u32)];
    for _ in 0..read::MAX_ITEMS {
        let (destination, source, offset) = *frames.last().unwrap();
        let value = byte(vm, source.wrapping_add(offset))?;
        if matches!(value, 0 | b')') {
            let end = if value == b')' {
                vm.write(destination.wrapping_add(offset), b")")?;
                offset.wrapping_add(1)
            } else {
                offset
            };
            vm.write(destination.wrapping_add(end), &[0; 2])?;
            frames.pop();
            if let Some(frame) = frames.last_mut() {
                frame.2 = frame.2.wrapping_add(end);
            } else {
                return Ok(end);
            }
        } else if value == b'(' {
            let destination = destination.wrapping_add(offset);
            let source = source.wrapping_add(offset);
            vm.write(destination, b"(")?;
            frames.push((destination, source, 1));
        } else {
            vm.write(destination.wrapping_add(offset), &[value])?;
            frames.last_mut().unwrap().2 = offset.wrapping_add(1);
        }
    }
    Err(Error::new(vm.pc, "KCS 括号复制超过上限"))
}

pub(super) fn split_parenthesized(
    vm: &mut Interpreter,
    destination: u32,
    source: u32,
    separators: u32,
) -> Result<()> {
    let mut input = 0u32;
    let mut output = 0u32;
    let mut separated = true;
    let mut previous = 0u8;
    for _ in 0..read::MAX_ITEMS {
        let value = byte(vm, source.wrapping_add(input))?;
        if value == 0 {
            return vm.write(destination.wrapping_add(output), &[0; 2]);
        }
        if value == b'(' {
            let marker = byte(vm, source.wrapping_add(input).wrapping_add(1))?;
            if previous.is_ascii_alphabetic() && !separated {
                vm.write(destination.wrapping_add(output), &[0])?;
                output = output.wrapping_add(1);
            }
            let length = copy_parentheses(
                vm,
                destination.wrapping_add(output),
                source.wrapping_add(input),
            )?;
            if marker == b':' {
                if length > 3 {
                    let content = vm
                        .read(
                            destination.wrapping_add(output).wrapping_add(2),
                            (length - 3) as usize,
                        )?
                        .to_vec();
                    vm.write(destination.wrapping_add(output), &content)?;
                }
                output = output.wrapping_add(length);
                if length >= 2 {
                    vm.write(destination.wrapping_add(output).wrapping_sub(3), &[0])?;
                }
                output = output.wrapping_sub(3);
            } else {
                output = output.wrapping_add(length);
            }
            separated = false;
            input = input.wrapping_add(length);
        } else {
            let mut cursor = separators;
            let separator = loop {
                let separator = byte(vm, cursor)?;
                if separator == 0 || separator == value {
                    break separator != 0;
                }
                cursor = cursor.wrapping_add(1);
            };
            if !separator {
                vm.write(destination.wrapping_add(output), &[value])?;
                output = output.wrapping_add(1);
                separated = false;
                previous = value;
            } else if !separated {
                vm.write(destination.wrapping_add(output), &[0])?;
                output = output.wrapping_add(1);
                separated = true;
                previous = 0;
            }
            input = input.wrapping_add(1);
        }
    }
    Err(Error::new(vm.pc, "KCS 括号分词超过上限"))
}
