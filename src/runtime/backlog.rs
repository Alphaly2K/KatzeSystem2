use super::{ObjectFactory, ObjectHost};
use crate::backlog::{Backlog, Record};
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn backlog_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 696 {
            if self.resources.backlogs.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "回看日志数量超过上限"));
            }
            let settings = self
                .host
                .system_settings
                .as_ref()
                .map(|s| s.backlog.clone())
                .unwrap_or_default();
            let handle = self.host.handle_allocator().allocate()?;
            self.resources
                .backlogs
                .insert(handle, Backlog::new(settings));
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if handle == 0 {
            return Ok(if matches!(index, 850 | 851) {
                NativeReturn::Value(0)
            } else {
                NativeReturn::Continue
            });
        }
        let length = match index {
            697 | 876 => 4,
            785 => 8,
            700 | 850 | 851 => 12,
            699 => 16,
            1034 => 20,
            698 => 28,
            977 => 32,
            _ => unreachable!(),
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if matches!(index, 699 | 1034) {
            return self.import_backlog(index, arguments, vm);
        }
        if matches!(index, 850 | 851) {
            let log = read::u32_at(arguments, 8)?;
            if log == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let record = read::u32_at(arguments, 4)?;
            if index == 850 {
                let data = self
                    .resources
                    .backlogs
                    .get(&log)
                    .ok_or_else(|| Error::new(vm.pc, "回看日志句柄无效"))?
                    .serialize()?;
                self.host
                    .data_container_mut(handle)?
                    .append(record, &data)?;
            } else {
                let Some(data) = self.host.data_container(handle)?.record(record)? else {
                    return Ok(NativeReturn::Value(0));
                };
                self.resources
                    .backlogs
                    .get_mut(&log)
                    .ok_or_else(|| Error::new(vm.pc, "回看日志句柄无效"))?
                    .restore(data)?;
            }
            return Ok(NativeReturn::Value(1));
        }
        if index == 697 {
            if self.resources.backlogs.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "回看日志句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        let log = self
            .resources
            .backlogs
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "回看日志句柄无效"))?;
        match index {
            698 | 977 => {
                let file = formatted(vm, arguments, 4, 64)?;
                let name = formatted(vm, arguments, 12, 256)?;
                let text = formatted(vm, arguments, 16, 4096)?;
                let name = name
                    .windows(3)
                    .position(|v| v == [0xef, 0xbc, 0xa0])
                    .map_or(name.as_slice(), |at| &name[at + 3..]);
                log.push(Record {
                    file: &file,
                    input: read::u32_at(arguments, 8)?,
                    name,
                    text: &text,
                    flag: read::u32_at(arguments, 20)?,
                    space: read::u32_at(arguments, 24)?,
                    extra: if index == 977 {
                        read::u32_at(arguments, 28)?
                    } else {
                        u32::MAX
                    },
                })?;
            }
            700 => {
                let address = read::u32_at(arguments, 4)?;
                vm.write(address, &[0])?;
                if let Some(name) = log.name_for_row(read::u32_at(arguments, 8)?) {
                    let mut name = name.to_vec();
                    name.push(0);
                    vm.write(address, &name)?;
                }
            }
            785 => log.set_limit(read::u32_at(arguments, 4)?),
            876 => log.clear(),
            _ => unreachable!(),
        }
        Ok(NativeReturn::Continue)
    }

    fn import_backlog(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let context = read::u32_at(arguments, 4)?;
        if context == 0 {
            return Ok(NativeReturn::Continue);
        }
        let history: Vec<_> = self
            .resources
            .cst_contexts
            .get(&context)
            .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?
            .history()
            .map(|(name, count)| (name.to_vec(), count))
            .collect();
        let log = self
            .resources
            .backlogs
            .get_mut(&read::u32_at(arguments, 0)?)
            .ok_or_else(|| Error::new(vm.pc, "回看日志句柄无效"))?;
        log.clear();
        let voice = formatted(vm, arguments, 8, 64)?;
        let continuation = formatted(vm, arguments, 12, 64)?;
        let language = if index == 1034 {
            Some(formatted(vm, arguments, 16, 64)?)
        } else {
            None
        };
        let mut start = 0;
        if log.limit() != u32::MAX {
            let mut total = 0i32;
            let mut cursor = history.len().saturating_sub(1);
            while cursor > 0 && total < log.limit() as i32 {
                start = cursor;
                total = total.wrapping_add(history[cursor].1.wrapping_sub(1) as i32);
                cursor -= 1;
            }
            if cursor == 0 && total < log.limit() as i32 {
                start = 0;
            }
        }
        // 导入入口直接构造临时场景，不执行 323 的全局语言登记。
        let mut scene = super::CstContext::new(&[]);
        let mut continued = false;
        for (entry, (file, count)) in history.iter().enumerate().skip(start) {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置 CST 资源服务"))?;
            let resolved = services.resolve(b"scene", file)?;
            if !scene.load(&resolved, services)? {
                continue;
            }
            if *count as i32 > read::MAX_ITEMS as i32 {
                return Err(Error::new(vm.pc, "回看导入输入段数量超过上限"));
            }
            for input in 0..(*count as i32).max(0) {
                let mut name = Vec::new();
                let mut body = Vec::new();
                let mut buffer = Vec::new();
                let mut flag = false;
                let mut space = !continued;
                let script = scene.script().unwrap();
                let records = script
                    .inputs()
                    .get(input as usize)
                    .map(|range| &script.records()[range.clone()])
                    .unwrap_or_default();
                for (record, line) in records.iter().enumerate() {
                    // 本段已有声音标记时，后续命令仍使用最近一次读取的文本缓冲。
                    if matches!(line.kind, 0x20 | 0x21) || (line.kind == 0x30 && !flag) {
                        buffer = scene
                            .record_text(
                                input,
                                record as i32,
                                language.as_deref(),
                                index == 1034,
                                self.host.unicode_text != 0,
                            )?
                            .unwrap_or_default();
                        check_capacity(&buffer, 4096)?;
                    }
                    match line.kind {
                        0x20 => body.extend_from_slice(&buffer),
                        0x21 => {
                            let marker: &[u8] = if index == 1034 {
                                "＠".as_bytes()
                            } else {
                                b"\x81\x97"
                            };
                            let at = buffer
                                .windows(marker.len())
                                .position(|b| b == marker)
                                .map_or(0, |at| at + marker.len());
                            name.extend_from_slice(&buffer[at..]);
                        }
                        0x30 => {
                            if !flag && has_voice(&voice, &buffer, services)? {
                                flag = true;
                                continue;
                            }
                            continued =
                                continuation_flag(&continuation, &buffer, continued, self.numbers)?;
                        }
                        2 => space = !continued,
                        3 if continued => space = true,
                        _ => {}
                    }
                    check_capacity(&name, 256)?;
                    check_capacity(&body, 4096)?;
                }
                body = crate::text::expand(&body, &self.numbers.strings)?;
                name = crate::text::expand(&name, &self.numbers.strings)?;
                check_capacity(&body, 4096)?;
                check_capacity(&name, 256)?;
                log.push(Record {
                    file,
                    input: input as u32,
                    name: &name,
                    text: &body,
                    flag: u32::from(flag),
                    space: u32::from(space),
                    extra: entry as u32,
                })?;
            }
        }
        Ok(NativeReturn::Continue)
    }
}

fn check_capacity(text: &[u8], capacity: usize) -> Result<()> {
    if text.len() >= capacity {
        return Err(Error::new(0, "回看导入字串超过缓冲区上限"));
    }
    Ok(())
}

fn has_voice(
    prefix: &[u8],
    command: &[u8],
    services: &mut dyn crate::kcs::ResourceServices,
) -> Result<bool> {
    let mut words = command
        .split(|&b| b == b' ')
        .filter(|word| !word.is_empty());
    if words.next() != Some(prefix) {
        return Ok(false);
    }
    for word in words {
        let name = services.resolve(b"sound", word)?;
        if services
            .read(&name, crate::resource::ResourceMode::LooseFirst)?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn continuation_flag(
    prefix: &[u8],
    command: &[u8],
    previous: bool,
    numbers: &mut crate::expression::State,
) -> Result<bool> {
    let end = command
        .iter()
        .position(|&b| b == b' ')
        .unwrap_or(command.len());
    if !prefix
        .get(..end)
        .is_some_and(|word| word.eq_ignore_ascii_case(&command[..end]))
        || end == command.len()
    {
        return Ok(previous);
    }
    let parameter = command[end + 1..].split(|&b| b == b' ').next().unwrap();
    check_capacity(parameter, 256)?;
    Ok(crate::expression::eval_fes_fallback(crate::text::ascii(parameter)?, numbers)? != 0)
}

fn formatted(
    vm: &Interpreter,
    arguments: &[u8],
    offset: usize,
    capacity: usize,
) -> Result<Vec<u8>> {
    let mut text = format_string(vm, read::u32_at(arguments, offset)?, 0)?;
    text.truncate(text.iter().position(|&b| b == 0).unwrap_or(text.len()));
    if text.len() >= capacity {
        return Err(Error::new(vm.pc, "回看日志格式字串超过缓冲区上限"));
    }
    Ok(text)
}
