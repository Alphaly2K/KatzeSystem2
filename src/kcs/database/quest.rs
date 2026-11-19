use super::{table::Table, text};
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, ResourceState, native, strings};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

pub(in crate::kcs) struct QuestTable {
    table: Table,
    rows: BTreeMap<i32, usize>,
    state: [u32; 2],
}

impl QuestTable {
    fn parse(bytes: &[u8]) -> Result<Option<Self>> {
        let Some(table) = Table::parse_case_insensitive(bytes)? else {
            return Ok(None);
        };
        let mut rows = BTreeMap::new();
        for row in 0..table.len() {
            let id = cell_word(&table, row, b"TBL")? as i32;
            rows.entry(id).or_insert(row);
        }
        Ok(Some(Self {
            table,
            rows,
            state: [0; 2],
        }))
    }

    fn position(&self, id: i32, mut ordinal: u32) -> Result<u32> {
        if let Some(&row) = self.rows.get(&id) {
            for slot in 0..20u8 {
                let name = [b'P', b'T', b'0' + slot / 10, b'0' + slot % 10];
                if cell_word(&self.table, row, &name)? != 0 {
                    if ordinal == 0 {
                        return Ok(u32::from(slot));
                    }
                    ordinal -= 1;
                }
            }
        }
        Ok(u32::MAX)
    }
}

fn cell_word(table: &Table, row: usize, column: &[u8]) -> Result<u32> {
    let mut word = [0; 4];
    if let Some(bytes) = table.cell(row, column) {
        word.get_mut(..bytes.len())
            .ok_or_else(|| Error::new(row, "任务记录表数值字段超过四字节"))?
            .copy_from_slice(bytes);
    }
    Ok(u32::from_le_bytes(word))
}

impl ResourceState {
    pub(crate) fn quest_table_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        if index == 795 {
            let args = native::words(arguments, 1, vm.pc)?;
            let name = strings::format(vm, args[0], 0)?;
            let name = text(&name);
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "任务记录表名称超过固定缓冲区上限"));
            }
            let services = services.ok_or_else(|| Error::new(vm.pc, "任务记录表需要资源服务"))?;
            let name = services.resolve(b"database", name)?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let Some(table) = QuestTable::parse(&bytes)? else {
                return Ok(NativeReturn::Value(0));
            };
            let handle = self.allocate_handle(vm.pc)?;
            self.quest_tables.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }

        let size = match index {
            796 | 798 | 800 => 4,
            797 | 799 => 8,
            805 => 12,
            _ => unreachable!(),
        };
        let handle = read::u32_at(arguments, 0)?;
        if arguments.len() != size
            && !(handle == 0
                && (4..size).contains(&arguments.len())
                && arguments.len().is_multiple_of(4))
        {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if handle == 0 {
            return Ok(match index {
                798 | 800 => NativeReturn::Value(0),
                805 => NativeReturn::Value(u32::MAX),
                _ => NativeReturn::Continue,
            });
        }
        let invalid = || Error::new(vm.pc, "KCS 任务记录表句柄无效");
        if index == 796 {
            self.quest_tables.remove(&handle).ok_or_else(invalid)?;
            return Ok(NativeReturn::Continue);
        }
        let table = self.quest_tables.get_mut(&handle).ok_or_else(invalid)?;
        match index {
            797 | 799 => {
                table.state[usize::from(index == 799)] = read::u32_at(arguments, 4)?;
                Ok(NativeReturn::Continue)
            }
            798 | 800 => Ok(NativeReturn::Value(table.state[usize::from(index == 800)])),
            805 => Ok(NativeReturn::Value(table.position(
                read::u32_at(arguments, 4)? as i32,
                read::u32_at(arguments, 8)?,
            )?)),
            _ => unreachable!(),
        }
    }
}
