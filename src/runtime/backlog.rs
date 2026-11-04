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
            698 => 28,
            977 => 32,
            _ => unreachable!(),
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
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
                    .position(|v| v == [0xe2, 0x80, 0xbb])
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
