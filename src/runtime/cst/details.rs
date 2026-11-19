use super::{CstContext, Entry};
use crate::kcs::{Interpreter, NativeReturn};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::{Error, Result, read, text};

impl Entry {
    fn set_string(&mut self, position: u32, id: u32, value: &[u8]) -> Result<()> {
        let value = terminated(value);
        let offset = self.details.chunks_exact(72).position(|record| {
            read::u32_at(record, 0) == Ok(position) && read::u32_at(record, 4) == Ok(id)
        });
        let offset = if let Some(offset) = offset {
            offset * 72
        } else {
            if self.details.len() > read::MAX_BYTES - 72 {
                return Err(Error::new(0, "CST 字符串记录超过读取上限"));
            }
            let offset = self.details.len();
            self.details.resize(offset + 72, 0);
            self.details[offset..offset + 4].copy_from_slice(&position.to_le_bytes());
            self.details[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
            offset
        };
        let target = &mut self.details[offset + 8..offset + 72];
        let length = value.len().min(62);
        target[..length].copy_from_slice(&value[..length]);
        target[length] = 0;
        if value.len() > 62 {
            target[63] = 0;
        }
        Ok(())
    }

    fn string(&self, position: u32, id: u32) -> Result<Option<&[u8]>> {
        let word = |record: &[u8], offset| read::u32_at(record, offset);
        let records = || self.details.chunks_exact(72);
        let exact =
            records().find(|record| word(record, 0) == Ok(position) && word(record, 4) == Ok(id));
        let record = exact.or_else(|| {
            let previous = records()
                .filter_map(|record| word(record, 0).ok())
                .filter(|&candidate| candidate < position)
                .max()
                .unwrap_or(0);
            records().find(|record| word(record, 0) == Ok(previous) && word(record, 4) == Ok(id))
        });
        let Some(record) = record else {
            return Ok(None);
        };
        let bytes = &record[8..];
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| Error::new(0, "CST 字符串记录缺少终止零"))?;
        Ok(Some(&bytes[..end]))
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_detail_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let legacy = index < 333;
        let index = if legacy { index + 105 } else { index };
        let empty = || {
            Ok(if matches!(index, 336 | 337) {
                NativeReturn::Continue
            } else {
                NativeReturn::Value(0)
            })
        };
        let (context, arguments) = if legacy {
            let Some(context) = self.resources.legacy_cst.get_mut(&self.host.task_id) else {
                return empty();
            };
            (context, arguments)
        } else {
            let handle = read::u32_at(arguments, 0)?;
            if handle == 0 {
                let size = if matches!(index, 334 | 335) { 20 } else { 16 };
                if arguments.len() != 4 && arguments.len() != size {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                return empty();
            }
            let context = self
                .resources
                .cst_contexts
                .get_mut(&handle)
                .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?;
            (context, &arguments[4..])
        };
        let size = if matches!(index, 334 | 335) { 16 } else { 12 };
        if arguments.len() != size {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let selected = context.history_index(read::u32_at(arguments, 0)?);
        if index == 333 {
            let entry = context.history.get(selected);
            let (name, value) =
                entry.map_or((&b""[..], u32::MAX), |entry| (entry.name(), entry.value));
            let mut name = name.to_vec();
            name.push(0);
            vm.write(read::u32_at(arguments, 4)?, &name)?;
            vm.write(read::u32_at(arguments, 8)?, &value.to_le_bytes())?;
            return Ok(NativeReturn::Value(
                entry.map_or(u32::MAX, |_| selected as u32),
            ));
        }
        let position = read::u32_at(arguments, 4)?;
        if matches!(index, 334 | 335) {
            let address = read::u32_at(arguments, 12)?;
            let id = read::u32_at(arguments, 8)?;
            if index == 334 {
                let Some(entry) = context.history.get_mut(selected) else {
                    return Ok(NativeReturn::Value(0));
                };
                let value = vm.read_string(address)?;
                entry.set_string(position, id, &value)?;
                return Ok(NativeReturn::Value(1));
            }
            let Some(entry) = context
                .history
                .get(selected)
                .filter(|entry| !entry.details.is_empty())
            else {
                return Ok(NativeReturn::Value(0));
            };
            let value = entry.string(position, id)?;
            let found = value.is_some();
            let mut bytes = value.unwrap_or_default().to_vec();
            bytes.push(0);
            vm.write(address, &bytes)?;
            return Ok(NativeReturn::Value(u32::from(found)));
        }
        let address = read::u32_at(arguments, 8)?;
        if index == 337 {
            let output = expand_history_strings(
                context.history.get(selected),
                position,
                &vm.read_string(address)?,
                false,
                vm.pc,
            )?;
            vm.write(address, &output)?;
        } else {
            let input = vm.read_string(address)?;
            let mut offset = 0;
            while offset < input.len() {
                let marker = input[offset..]
                    .strip_prefix(b"$str")
                    .and_then(text::integer_prefix);
                if let Some((length, id)) = marker {
                    if let (Some(entry), Some(value)) = (
                        context.history.get_mut(selected),
                        self.numbers.strings.get(&id),
                    ) {
                        entry.set_string(position, id, value)?;
                        offset += 4 + length;
                        continue;
                    }
                }
                offset += 1;
            }
        }
        Ok(NativeReturn::Continue)
    }
}

impl CstContext {
    pub(super) fn history_index(&self, requested: u32) -> usize {
        (requested as usize).min(self.history.len().saturating_sub(1))
    }
}

pub(super) fn expand_history_strings(
    entry: Option<&Entry>,
    position: u32,
    input: &[u8],
    preserve_missing: bool,
    pc: usize,
) -> Result<Vec<u8>> {
    let mut offset = 0;
    let mut output = Vec::new();
    while offset < input.len() {
        let marker = input[offset..]
            .strip_prefix(b"$str")
            .and_then(text::integer_prefix);
        if let Some((length, id)) = marker {
            let end = offset + 4 + length;
            if let Some(value) = entry
                .map(|entry| entry.string(position, id))
                .transpose()?
                .flatten()
            {
                output.extend(value);
            } else if preserve_missing {
                output.extend_from_slice(&input[offset..end]);
            }
            offset = end;
        } else {
            output.push(input[offset]);
            offset += 1;
        }
        if output.len() > read::MAX_BYTES - 2 {
            return Err(Error::new(pc, "CST 字符串替换超过读取上限"));
        }
    }
    output.extend([0, 0]);
    Ok(output)
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}
