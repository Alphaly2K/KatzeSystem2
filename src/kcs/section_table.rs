use super::{Interpreter, NativeReturn, ResourceServices, ResourceState, strings};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct SectionTable {
    sections: BTreeMap<Vec<u8>, Vec<Entry>>,
}

struct Entry {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl SectionTable {
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "节表超过读取上限"));
        }
        let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        let mut table = Self::default();
        let mut section = None;
        let mut block_comment = false;
        let mut count = 0;
        for raw in bytes.split(|&b| matches!(b, b'\r' | b'\n')) {
            let mut line = Vec::new();
            let mut p = 0;
            while p < raw.len() {
                if block_comment {
                    if raw[p..].starts_with(b"*/") {
                        block_comment = false;
                        p += 2;
                    } else {
                        p += 1;
                    }
                } else if raw[p..].starts_with(b"//") {
                    break;
                } else if raw[p..].starts_with(b"/*") {
                    block_comment = true;
                    // 开始符号的星号仍参与结束符号检查。
                    p += 1;
                } else {
                    let byte = if raw[p] == b'\t' { b' ' } else { raw[p] };
                    if byte != b' ' || (!line.is_empty() && line.last() != Some(&b' ')) {
                        line.push(byte);
                    }
                    p += 1;
                }
            }
            if line.last() == Some(&b' ') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            if line[0] == b'#' {
                table.sections.entry(line.clone()).or_default();
                section = Some(line);
            } else if let Some(name) = &section {
                let split = line.iter().position(|&b| b == b' ').unwrap_or(line.len());
                let value = &line[(split + 1).min(line.len())..];
                if split >= 1024 || value.len() >= 1024 {
                    return Err(Error::new(0, "节表字段超过固定缓冲区上限"));
                }
                table.sections.get_mut(name).unwrap().push(Entry {
                    key: line[..split].to_vec(),
                    value: value.to_vec(),
                });
            } else {
                continue;
            }
            count += 1;
            if count > read::MAX_ITEMS {
                return Err(Error::new(0, "节表条目超过上限"));
            }
        }
        Ok(table)
    }

    fn query(&self, section: &[u8], key: &[u8], occurrence: u32) -> Option<&[u8]> {
        self.sections
            .get(section)?
            .iter()
            .filter(|entry| entry.key == key)
            .nth(occurrence as usize)
            .map(|entry| entry.value.as_slice())
    }
}

impl ResourceState {
    pub(crate) fn section_table_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        if index == 685 && arguments == [0; 4] {
            return Ok(NativeReturn::Value(0));
        }
        let args = super::native::words(arguments, if index == 685 { 5 } else { 1 }, vm.pc)?;
        if index == 683 {
            let mut name = strings::format(vm, args[0], 0)?;
            name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let table = SectionTable::parse(&bytes)?;
            let handle = self.allocate_handle(vm.pc)?;
            self.section_tables.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 684 {
            if args[0] != 0 && self.section_tables.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS 节表句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let table = self
            .section_tables
            .get(&args[0])
            .ok_or_else(|| Error::new(vm.pc, "KCS 节表句柄无效"))?;
        let section = strings::format(vm, args[1], 0)?;
        let key = strings::format(vm, args[2], 0)?;
        if vm.read(args[4], 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        let Some(value) = table.query(&section, &key, args[3]) else {
            return Ok(NativeReturn::Value(0));
        };
        super::native::write_string(vm, args[4], value, 1)?;
        Ok(NativeReturn::Value(1))
    }
}
