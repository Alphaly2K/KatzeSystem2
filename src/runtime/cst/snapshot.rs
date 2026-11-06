use crate::kcs::{Interpreter, NativeReturn};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::{Error, Result, read};
use std::collections::BTreeMap;
use std::io::Write;

fn header(bytes: &[u8], offset: usize) -> Result<[u32; 5]> {
    let bytes = read::range(bytes, offset, 20)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn next(bytes: &[u8], offset: usize, distance: u32) -> Result<Option<usize>> {
    if distance == 0 {
        return Ok(None);
    }
    let offset = offset
        .checked_add(distance as usize)
        .filter(|&offset| distance >= 20 && offset <= bytes.len().saturating_sub(20))
        .ok_or_else(|| Error::new(offset, "CST 快照链偏移越界"))?;
    Ok(Some(offset))
}

fn encode(
    previous: &[u8],
    position: u32,
    number_ids: &[u32],
    string_ids: &[u32],
    numbers: &crate::expression::State,
    available: bool,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for &id in number_ids {
        bytes.extend(id.to_le_bytes());
        bytes.extend(
            (if available {
                numbers.shared.get(id as usize).copied().unwrap_or(0)
            } else {
                0
            })
            .to_le_bytes(),
        );
    }
    let number_size = bytes.len();
    let strings: BTreeMap<_, _> = string_ids
        .iter()
        .map(|&id| {
            let value = numbers.strings.get(&id).map_or(&[][..], Vec::as_slice);
            let value = &value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())];
            (id, value)
        })
        .collect();
    bytes.extend((strings.len() as u32).to_le_bytes());
    for (id, value) in strings {
        if bytes.len().saturating_add(value.len()) > read::MAX_BYTES - 13 {
            return Err(Error::new(0, "CST 快照字符串超过读取上限"));
        }
        bytes.extend(id.to_le_bytes());
        bytes.extend(value);
        bytes.push(0);
    }
    // 原字符串序列化长度还包含八个未写入字节；确定性输出将它们置零。
    bytes.extend([0; 8]);
    let string_size = bytes.len() - number_size;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder
        .write_all(&bytes)
        .map_err(|e| Error::new(0, e.to_string()))?;
    let compressed = encoder.finish().map_err(|e| Error::new(0, e.to_string()))?;
    let block_size = 20 + compressed.len();
    let size = block_size
        .checked_add(previous.len())
        .filter(|&n| n <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(0, "CST 快照链超过读取上限"))?;
    let mut output = Vec::with_capacity(size);
    for word in [
        if previous.is_empty() {
            0
        } else {
            block_size as u32
        },
        block_size as u32,
        number_size as u32,
        string_size as u32,
        position,
    ] {
        output.extend(word.to_le_bytes());
    }
    output.extend(compressed);
    output.extend(previous);
    Ok(output)
}

fn selection(ranges: &[[u32; 2]; 8]) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    for &[start, count] in ranges {
        let end = start.wrapping_add(count) as i32;
        let count = (i64::from(end) - i64::from(start as i32)).max(0) as usize;
        if count > read::MAX_ITEMS - result.len() {
            return Err(Error::new(0, "CST 快照数值范围超过上限"));
        }
        result.extend((0..count).map(|i| start.wrapping_add(i as u32)));
    }
    Ok(result)
}

fn restore(
    bytes: &[u8],
    h: [u32; 5],
    offset: usize,
    ids: &[u32],
    state: &mut crate::expression::State,
    available: bool,
) -> Result<()> {
    let number_size = h[2] as usize;
    if !number_size.is_multiple_of(8) {
        return Err(Error::new(offset + 8, "CST 数值快照长度不是八字节的倍数"));
    }
    let string_size = h[3] as usize;
    let size = number_size
        .checked_add(string_size)
        .filter(|&n| n <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(offset, "CST 快照正文超过读取上限"))?;
    let compressed_size = (h[1] as usize)
        .checked_sub(20)
        .ok_or_else(|| Error::new(offset + 4, "CST 快照压缩边界无效"))?;
    let data = read::inflate(read::range(bytes, offset + 20, compressed_size)?, size)?;
    for pair in data[..number_size].chunks_exact(8) {
        let id = read::u32_at(pair, 0)? as usize;
        if let Some(target) = state.shared.get_mut(id).filter(|_| available) {
            *target = read::u32_at(pair, 4)? as i32;
        }
    }
    let strings = &data[number_size..];
    let count = read::u32_at(strings, 0)? as usize;
    if count > read::MAX_ITEMS {
        return Err(Error::new(offset, "CST 快照字符串数量超过上限"));
    }
    let mut saved = BTreeMap::new();
    let mut at = 4;
    for _ in 0..count {
        let id = read::u32_at(strings, at)?;
        at += 4;
        let rest = read::range(strings, at, strings.len() - at)?;
        let length = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::new(at, "CST 快照字符串缺少结束零"))?;
        saved.entry(id).or_insert(&rest[..length]);
        at += length + 1;
    }
    for id in ids {
        state
            .strings
            .insert(*id, saved.get(id).copied().unwrap_or_default().to_vec());
    }
    Ok(())
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_snapshot_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        let count = if index == 992 { 4 } else { 3 };
        if arguments.len() != count * 4 && !(handle == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if handle == 0 {
            return Ok(if index == 992 {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        if !self.resources.cst_contexts.contains_key(&handle) {
            return Err(Error::new(vm.pc, "CST 上下文句柄无效"));
        }
        if index == 994 {
            let requested = read::u32_at(arguments, 4)?;
            let position = read::u32_at(arguments, 8)?;
            let context = self.resources.cst_contexts.get_mut(&handle).unwrap();
            let mut remaining = None;
            if let Some(entry) = context
                .history
                .get(requested as usize)
                .filter(|entry| !entry.saved_data.is_empty())
            {
                let mut offset = 0;
                loop {
                    let h = header(&entry.saved_data, offset)?;
                    if (h[4] as i32) < position as i32 {
                        if offset != 0 {
                            remaining = Some(entry.saved_data[offset..].to_vec());
                        }
                        break;
                    }
                    let Some(at) = next(&entry.saved_data, offset, h[0])? else {
                        break;
                    };
                    offset = at;
                }
            }
            // 原写入入口更新末条历史，即使请求裁剪的是较早的条目。
            if let (Some(data), Some(last)) = (remaining, context.history.last_mut()) {
                last.saved_data = data;
            }
            if let Some(entry) = context.history.get_mut(requested as usize) {
                entry.value = position;
            }
            let after = requested.wrapping_add(1);
            if after != u32::MAX {
                context.history.truncate((after as i32).max(0) as usize);
            }
            return Ok(NativeReturn::Continue);
        }
        let format = read::u32_at(arguments, if index == 991 { 8 } else { 12 })?;
        let mut name = crate::kcs::format_string(vm, format, 0)?;
        name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
        if name.len() >= 1024 {
            return Err(Error::new(vm.pc, "CST 快照配置名超过缓冲区上限"));
        }
        let services = self
            .resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置 CST 资源服务"))?;
        let name = services.resolve(b"config", &name)?;
        let mut ids = Vec::new();
        if let Some(data) = services.read(&name, crate::resource::ResourceMode::LooseFirst)? {
            if let Ok(xml) = crate::resource::XmlDocument::parse(&data) {
                if let Some(text) = xml.text(b"localstr")? {
                    if text.len() >= 2048 {
                        return Err(Error::new(vm.pc, "CST 快照字符串配置超过缓冲区上限"));
                    }
                    for token in text.split(|&b| b == b',').filter(|token| !token.is_empty()) {
                        ids.push(crate::text::integer_prefix(token).map_or(0, |v| v.1));
                    }
                }
            }
        }
        let settings = self
            .host
            .system_settings
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
        let number_ids = selection(&settings.local_flag_ranges)?;
        let available = self
            .running
            .get(&self.host.task_id)
            .and_then(|task| task.shared_numbers)
            .unwrap_or(true);
        let context = self.resources.cst_contexts.get_mut(&handle).unwrap();
        if index == 991 {
            let previous = context
                .history
                .last()
                .map_or(&[][..], |entry| entry.saved_data.as_slice());
            let data = encode(
                previous,
                read::u32_at(arguments, 4)?,
                &number_ids,
                &ids,
                self.numbers,
                available,
            )?;
            if let Some(entry) = context.history.last_mut() {
                entry.saved_data = data;
            }
            return Ok(NativeReturn::Continue);
        }
        let input_address = read::u32_at(arguments, 4)?;
        let position_address = read::u32_at(arguments, 8)?;
        let requested = read::u32_at(&vm.read(input_address, 4)?, 0)? as i32;
        let position = read::u32_at(&vm.read(position_address, 4)?, 0)? as i32;
        if requested < 0 {
            return Ok(NativeReturn::Value(0));
        }
        for selected in (0..context.history.len().min(requested as usize + 1)).rev() {
            let data = &context.history[selected].saved_data;
            if data.is_empty() {
                continue;
            }
            let mut offset = 0;
            loop {
                let h = header(data, offset)?;
                if selected != requested as usize || h[4] as i32 <= position {
                    vm.write(input_address, &(selected as u32).to_le_bytes())?;
                    vm.write(position_address, &h[4].to_le_bytes())?;
                    restore(data, h, offset, &ids, self.numbers, available)?;
                    return Ok(NativeReturn::Value(1));
                }
                let Some(at) = next(data, offset, h[0])? else {
                    break;
                };
                offset = at;
            }
        }
        Ok(NativeReturn::Value(0))
    }
}
