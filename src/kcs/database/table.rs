use super::text;
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, ResourceState, native, strings};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

pub(in crate::kcs) struct Table {
    columns: BTreeMap<Vec<u8>, (usize, usize)>,
    rows: Vec<u8>,
    header: usize,
    count: usize,
    stride: usize,
}

impl Table {
    pub(in crate::kcs) fn parse(bytes: &[u8]) -> Result<Option<Self>> {
        if !bytes.starts_with(b"KCDB") {
            return Ok(None);
        }
        let packed = read::sized(read::u32_at(bytes, 4)?, 4)?;
        let plain = read::sized(read::u32_at(bytes, 8)?, 8)?;
        let Ok(rows) = read::inflate(read::range(bytes, 12, packed)?, plain) else {
            return Ok(None);
        };
        let count = read::u32_at(&rows, 0)? as usize;
        if count > read::MAX_ITEMS {
            return Err(Error::new(0, "数据库字段数量超过上限"));
        }
        let definitions = read::range(&rows, 4, count * 24)?;
        let mut columns = BTreeMap::new();
        let mut offset = 0usize;
        for column in definitions.chunks_exact(24) {
            let size = read::sized(read::u32_at(column, 20)?, 20)?;
            columns
                .entry(text(&column[..16]).to_vec())
                .or_insert((offset, size));
            offset = offset
                .checked_add(size)
                .filter(|&size| size <= read::MAX_BYTES)
                .ok_or_else(|| Error::new(4, "数据库行布局超过上限"))?;
        }
        let header = 4 + definitions.len();
        let count = read::u32_at(&rows, header)? as usize;
        let stride = read::sized(read::u32_at(&rows, header + 4)?, header + 4)?;
        if count > read::MAX_ITEMS || offset > stride {
            return Err(Error::new(header, "数据库行数量或布局无效"));
        }
        let length = count
            .checked_mul(stride)
            .filter(|&n| n <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(header, "数据库行数据超过上限"))?;
        read::range(&rows, header + 8, length)?;
        Ok(Some(Self {
            columns,
            rows,
            header,
            count,
            stride,
        }))
    }

    pub(super) fn width(&self, column: &[u8]) -> Option<usize> {
        self.columns.get(column).map(|&(_, size)| size)
    }

    pub(in crate::kcs) fn len(&self) -> usize {
        self.count
    }

    pub(super) fn header(&self) -> usize {
        self.header
    }

    pub(in crate::kcs) fn cell(&self, row: usize, column: &[u8]) -> Option<&[u8]> {
        let &(offset, size) = self.columns.get(column)?;
        if row >= self.count {
            return None;
        }
        let start = self.header + 8 + row * self.stride + offset;
        Some(&self.rows[start..start + size])
    }
}

impl ResourceState {
    pub(crate) fn database_table_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        let count = match index {
            791 | 792 => 1,
            793 => 3,
            794 => 4,
            _ => unreachable!(),
        };
        let args = native::words(arguments, count, vm.pc)?;
        if index == 791 {
            let name = strings::format(vm, args[0], 0)?;
            if text(&name).len() >= 260 {
                return Err(Error::new(vm.pc, "数据库名称超过固定缓冲区上限"));
            }
            let services = services.ok_or_else(|| Error::new(vm.pc, "数据库装载需要资源服务"))?;
            let name = services.resolve(b"database", text(&name))?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let Some(table) = Table::parse(&bytes)? else {
                return Ok(NativeReturn::Value(0));
            };
            let handle = self.allocate_handle(vm.pc)?;
            self.database_tables.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }
        let handle = args[0];
        if index == 792 {
            if handle != 0 && self.database_tables.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "KCS 数据库表句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if handle == 0 || index == 794 && vm.read(args[3], 1).is_err() {
            return Ok(NativeReturn::Value(0));
        }
        let table = self
            .database_tables
            .get(&handle)
            .ok_or_else(|| Error::new(vm.pc, "KCS 数据库表句柄无效"))?;
        let name = strings::format(vm, args[1], 0)?;
        let Some(cell) = table.cell(args[2] as usize, text(&name)) else {
            return Ok(NativeReturn::Value(0));
        };
        if index == 793 {
            let mut word = [0; 4];
            if cell.len() <= word.len() {
                word[..cell.len()].copy_from_slice(cell);
            }
            return Ok(NativeReturn::Value(u32::from_le_bytes(word)));
        }
        if !cell.is_empty() {
            vm.write(args[3], cell)?;
        }
        Ok(NativeReturn::Value(cell.len() as u32))
    }
}
