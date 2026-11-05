use super::{CstContext, Entry};
use crate::kcs::{Interpreter, NativeReturn};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::{Error, Result, read};

impl CstContext {
    fn encode_records(&self, history: bool, include_data: bool) -> Result<Vec<u8>> {
        let entries = if history {
            &self.history
        } else {
            &self.registrations
        };
        let length = entries.iter().try_fold(4usize, |length, entry| {
            length
                .checked_add(48)
                .and_then(|n| n.checked_add(entry.details.len()))
                .and_then(|n| {
                    n.checked_add(if include_data {
                        entry.saved_data.len()
                    } else {
                        0
                    })
                })
                .filter(|&n| n <= read::MAX_BYTES)
                .ok_or_else(|| Error::new(0, "CST 登记存档超过读取上限"))
        })?;
        let mut bytes = Vec::with_capacity(length);
        bytes.extend(0u32.to_le_bytes());
        for entry in entries {
            let mut header = [0; 48];
            header[..entry.name.len()].copy_from_slice(&entry.name);
            header[32..36].copy_from_slice(&entry.value.to_le_bytes());
            header[36..40].copy_from_slice(&((entry.details.len() / 72) as u32).to_le_bytes());
            if include_data && !entry.saved_data.is_empty() {
                header[40..44].copy_from_slice(&(entry.saved_data.len() as u32).to_le_bytes());
                header[44..48].copy_from_slice(&(48u32 + entry.details.len() as u32).to_le_bytes());
            }
            bytes.extend(header);
            bytes.extend_from_slice(&entry.details);
            if include_data {
                bytes.extend_from_slice(&entry.saved_data);
            }
        }
        Ok(bytes)
    }

    fn decode_records(&mut self, history: bool, bytes: &[u8]) -> Result<()> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "CST 登记存档超过读取上限"));
        }
        let current = read::u32_at(bytes, 0)? == 0;
        let mut offset = if current { 4 } else { 0 };
        let mut entries = Vec::new();
        while offset < bytes.len() {
            if entries.len() == read::MAX_ITEMS {
                return Err(Error::new(offset, "CST 登记数量超过上限"));
            }
            let header_size = if current { 48 } else { 40 };
            let header = read::range(bytes, offset, header_size)?;
            if !header[..32].contains(&0) {
                return Err(Error::new(offset, "CST 存档登记名称缺少终止零"));
            }
            let detail_size = (read::u32_at(header, 36)? as usize)
                .checked_mul(72)
                .filter(|&n| n <= read::MAX_BYTES)
                .ok_or_else(|| Error::new(offset + 36, "CST 存档附加记录超过上限"))?;
            let details = read::range(bytes, offset + header_size, detail_size)?.to_vec();
            let data_size = if current {
                read::u32_at(header, 40)? as usize
            } else {
                0
            };
            if !history && data_size != 0 {
                return Err(Error::new(
                    offset + 40,
                    "CST 登记恢复的附加数据分支使用无效基址",
                ));
            }
            let data = if data_size == 0 {
                Vec::new()
            } else {
                let start = offset
                    .checked_add(read::u32_at(header, 44)? as usize)
                    .ok_or_else(|| Error::new(offset + 44, "CST 存档数据偏移溢出"))?;
                read::range(bytes, start, data_size)?.to_vec()
            };
            let end = offset
                .checked_add(header_size)
                .and_then(|n| n.checked_add(detail_size))
                .and_then(|n| n.checked_add(data_size))
                .filter(|&n| n <= bytes.len())
                .ok_or_else(|| Error::new(offset, "CST 存档记录超出数据范围"))?;
            entries.push(Entry {
                name: header[..32].to_vec(),
                value: read::u32_at(header, 32)?,
                details,
                saved_data: data,
            });
            offset = end;
        }
        if history {
            self.history = entries;
        } else {
            self.registrations = entries;
        }
        Ok(())
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_record_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let length = if index == 385 { 20 } else { 16 };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let word = |i: usize| read::u32_at(arguments, i * 4);
        let container = word(0)?;
        let context = word(2)?;
        if container == 0 || context == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let record = word(1)?;
        let history = word(3)? != 0;
        if index == 385 {
            self.host.data_container(container)?;
            let data = self
                .resources
                .cst_contexts
                .get(&context)
                .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?
                .encode_records(history, word(4)? != 0)?;
            self.host
                .data_container_mut(container)?
                .append(record, &data)?;
        } else {
            let Some(data) = self.host.data_container(container)?.record(record)? else {
                return Ok(NativeReturn::Value(0));
            };
            self.resources
                .cst_contexts
                .get_mut(&context)
                .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?
                .decode_records(history, data)?;
        }
        Ok(NativeReturn::Value(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cst_record_reader_rejects_invalid_lengths_offsets_and_registration_data() {
        let mut context = CstContext::new(&[]);
        let mut bytes = vec![0; 52];
        bytes[4] = b'A';
        for input in [&[][..], &bytes[..51]] {
            assert!(context.decode_records(true, input).is_err());
        }
        bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(context.decode_records(true, &bytes).is_err());
        bytes[40..44].fill(0);
        bytes[44..48].copy_from_slice(&1u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(context.decode_records(true, &bytes).is_err());
        bytes[48..52].copy_from_slice(&48u32.to_le_bytes());
        bytes.push(7);
        assert!(
            context
                .decode_records(false, &bytes)
                .unwrap_err()
                .message
                .contains("无效基址")
        );
        context.decode_records(true, &bytes).unwrap();
        assert_eq!(context.history[0].saved_data, [7]);
    }
}
