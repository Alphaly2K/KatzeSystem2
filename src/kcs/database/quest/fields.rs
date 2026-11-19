use super::{COUNTERS, QuestTable, Table, cell_word, text};
use crate::{
    Error, Result,
    kcs::{Interpreter, native::write_string},
    text::integer_prefix,
};

pub(super) struct Characters {
    pub(super) ids: Vec<i32>,
    pub(super) parameters: Vec<Vec<i32>>,
}

impl Characters {
    pub(super) fn parse(table: &Table, row: usize) -> Result<Self> {
        let data = column_text(table, row, b"CHAR", 64)?;
        let mut ids = if let Some(list) = data.strip_prefix(b"[") {
            parse_group(list, row)?.0
        } else {
            Vec::new()
        };
        ids.sort_unstable();
        ids.dedup();
        let mut value = column_text(table, row, b"CPARAM", 64)?;
        let mut parameters = Vec::new();
        while let Some(list) = value.strip_prefix(b"[") {
            let (group, rest) = parse_group(list, row)?;
            parameters.push(group);
            value = rest;
        }
        if let Some(first) = parameters
            .first_mut()
            .filter(|first| first.first() == Some(&-1))
        {
            first.clone_from(&ids);
        }
        Ok(Self { ids, parameters })
    }
}

fn parse_group(mut bytes: &[u8], row: usize) -> Result<(Vec<i32>, &[u8])> {
    let mut group = Vec::new();
    while !bytes.is_empty() && !bytes.starts_with(b"]") {
        let (length, value) =
            integer_prefix(bytes).ok_or_else(|| Error::new(row, "任务记录表角色参数无法解析"))?;
        group.push(value as i32);
        bytes = &bytes[length..];
        bytes = bytes.strip_prefix(b",").unwrap_or(bytes);
    }
    Ok((group, bytes.strip_prefix(b"]").unwrap_or(bytes)))
}

fn column_text<'a>(table: &'a Table, row: usize, column: &[u8], cap: usize) -> Result<&'a [u8]> {
    let bytes = table.cell(row, column).unwrap_or_default();
    let text = text(bytes);
    if bytes.len() > cap || text.len() == cap {
        return Err(Error::new(row, "任务记录表文字超过字段容量"));
    }
    Ok(text)
}

pub(super) fn point(table: &Table, row: usize, slot: u8) -> Result<u32> {
    let name = [b'P', b'T', b'0' + slot / 10, b'0' + slot % 10];
    cell_word(table, row, &name)
}

impl QuestTable {
    pub(super) fn read_field(&self, index: u32, args: &[u32], vm: &mut Interpreter) -> Result<u32> {
        let address = args[args.len() - 1];
        let Some(record) = self.rows.get(&(args[1] as i32)) else {
            return Ok(0);
        };
        if vm.read(address, 1).is_err() {
            return Ok(0);
        }
        let row = record.row;
        let progress = record.progress;
        let number = |column| cell_word(&self.table, row, column);
        match (index, args[2]) {
            (804, _) => write_string(vm, address, column_text(&self.table, row, b"FILE", 64)?, 1)?,
            (806, slot @ 0..=2) => {
                let name = [b'Q', b'Z', b'T', b'B', b'L', b'1' + slot as u8];
                write_string(vm, address, column_text(&self.table, row, &name, 32)?, 1)?;
            }
            (806, 3) => {
                // The fourth selector starts at the adjacent PT words.
                let mut bytes = Vec::with_capacity(COUNTERS * 4 + 20);
                for slot in 0..COUNTERS as u8 {
                    bytes.extend(point(&self.table, row, slot)?.to_le_bytes());
                }
                for word in [
                    progress.flags,
                    progress.assignment,
                    progress.stamp,
                    progress.completed,
                    progress.elapsed,
                ] {
                    bytes.extend(word.to_le_bytes());
                }
                let text = crate::read::zstr(&bytes, 0)?;
                write_string(vm, address, text, 1)?;
            }
            (820, slot @ 0..=11) => {
                let value = match slot {
                    0 => number(b"TYPE")?,
                    1 => number(b"CLIENT")?,
                    2 => number(b"REWARD")?,
                    3 => number(b"LV_MIN")?,
                    4 => number(b"LV_MAX")?,
                    5 => number(b"COND")?,
                    6 => progress.assignment,
                    7 => progress.stamp,
                    8 => progress.completed,
                    9 => record.characters.ids.first().copied().unwrap_or(0) as u32,
                    10 => number(b"EXP")?,
                    11 => number(b"SECCNT")?,
                    _ => unreachable!(),
                };
                vm.write(address, &value.to_le_bytes())?;
            }
            _ => return Ok(0),
        }
        Ok(1)
    }
}
