use super::{table::Table, text};
use crate::kcs::{HostState, Interpreter, NativeReturn, ResourceServices, native, strings};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

mod save;

const UNASSIGNED: u32 = 0x8000_0000;
const COUNTERS: usize = 20;

#[derive(Clone, Copy)]
struct Progress {
    flags: u32,
    assignment: u32,
    stamp: u32,
    completed: u32,
    elapsed: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            flags: 0,
            assignment: UNASSIGNED,
            stamp: 0,
            completed: 0,
            elapsed: 0,
        }
    }
}

struct Record {
    row: usize,
    progress: Progress,
}

pub(in crate::kcs) struct QuestTable {
    table: Table,
    rows: BTreeMap<i32, Record>,
    state: [u32; 2],
    counters: [[u32; 6]; COUNTERS],
    attempts: u32,
}

impl QuestTable {
    fn parse(bytes: &[u8]) -> Result<Option<Self>> {
        let Some(table) = Table::parse_case_insensitive(bytes)? else {
            return Ok(None);
        };
        let mut rows = BTreeMap::new();
        for row in 0..table.len() {
            let id = cell_word(&table, row, b"TBL")? as i32;
            rows.entry(id).or_insert(Record {
                row,
                progress: Progress::default(),
            });
        }
        Ok(Some(Self {
            table,
            rows,
            state: [0; 2],
            counters: [[0; 6]; COUNTERS],
            attempts: 0,
        }))
    }

    fn position(&self, id: i32, mut ordinal: u32) -> Result<u32> {
        if let Some(record) = self.rows.get(&id) {
            for slot in 0..COUNTERS as u8 {
                let name = [b'P', b'T', b'0' + slot / 10, b'0' + slot % 10];
                if cell_word(&self.table, record.row, &name)? != 0 {
                    if ordinal == 0 {
                        return Ok(u32::from(slot));
                    }
                    ordinal -= 1;
                }
            }
        }
        Ok(u32::MAX)
    }

    fn count_progress(&self, args: &[u32], vm: &mut Interpreter) -> Result<()> {
        let [first, second] = [args[1], args[2]];
        let (min, max) = (args[3] as i32, args[4] as i32);
        vm.read(first, 4)?;
        vm.read(second, 4)?;
        vm.write(second, &[0; 4])?;
        vm.write(first, &[0; 4])?;
        if min <= max {
            // Each increment is visible through overlapping output addresses.
            for (_, record) in self.rows.range(min..=max) {
                for (address, value) in [
                    (first, record.progress.stamp),
                    (second, record.progress.completed),
                ] {
                    if value != 0 {
                        let current = read::u32_at(&vm.read(address, 4)?, 0)?;
                        vm.write(address, &current.wrapping_add(1).to_le_bytes())?;
                    }
                }
            }
        }
        Ok(())
    }

    fn find_assignment(&self, index: u32, args: &[u32]) -> Result<u32> {
        let mut ordinal = if index == 817 { args[2] } else { 0 };
        let assignment = args[1].wrapping_add(if index == 819 { 100 } else { 0 });
        for (&id, record) in &self.rows {
            if record.progress.assignment != assignment {
                continue;
            }
            if index != 817 {
                let kind = cell_word(&self.table, record.row, b"TYPE")?;
                if (index == 818 && !(2..=4).contains(&kind)) || (index == 819 && kind != 3) {
                    continue;
                }
            }
            if ordinal == 0 {
                return Ok(id as u32);
            }
            ordinal -= 1;
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

pub(crate) fn load(
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<Option<Vec<u8>>> {
    let args = native::words(arguments, 1, vm.pc)?;
    let name = strings::format(vm, args[0], 0)?;
    let name = text(&name);
    if name.len() >= 260 {
        return Err(Error::new(vm.pc, "任务记录表名称超过固定缓冲区上限"));
    }
    let services = services.ok_or_else(|| Error::new(vm.pc, "任务记录表需要资源服务"))?;
    let name = services.resolve(b"database", name)?;
    services.read(&name, ResourceMode::LooseFirst)
}

impl HostState {
    pub(crate) fn create_quest_table(&mut self, bytes: &[u8]) -> Result<u32> {
        let Some(table) = QuestTable::parse(bytes)? else {
            return Ok(0);
        };
        if self.quest_tables.len() >= read::MAX_ITEMS {
            return Err(Error::new(0, "任务记录表数量超过上限"));
        }
        let handle = self.handle_allocator().allocate()?;
        self.quest_tables.insert(handle, table);
        Ok(handle)
    }
}

pub(in crate::kcs) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    use NativeReturn::{Continue, Value};

    let count = match index {
        796 | 798 | 800 | 821 | 822 | 827 | 828 => 1,
        797 | 799 | 801 | 808 | 813 | 818 | 819 | 823 | 826 => 2,
        805 | 807 | 811 | 812 | 817 | 824 => 3,
        803 => 4,
        802 => 5,
        _ => unreachable!(),
    };
    let handle = read::u32_at(arguments, 0)?;
    if handle == 0 && !matches!(index, 802 | 803) {
        if !(4..=count * 4).contains(&arguments.len()) || !arguments.len().is_multiple_of(4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        return Ok(match index {
            798 | 800 | 823 | 824 | 826 => Value(0),
            805 | 817..=819 => Value(u32::MAX),
            _ => Continue,
        });
    }
    let args = native::words(arguments, count, vm.pc)?;
    let invalid = || Error::new(vm.pc, "KCS 任务记录表句柄无效");
    if index == 803 {
        let flags = if handle == 0 {
            [0, 0]
        } else {
            host.quest_tables
                .get(&handle)
                .ok_or_else(invalid)?
                .rows
                .get(&(args[1] as i32))
                .map_or([0, 0], |record| {
                    [record.progress.stamp, record.progress.completed]
                        .map(|word| u32::from(word != 0))
                })
        };
        vm.read(args[2], 4)?;
        vm.read(args[3], 4)?;
        vm.write(args[2], &flags[0].to_le_bytes())?;
        vm.write(args[3], &flags[1].to_le_bytes())?;
        return Ok(Continue);
    }
    if index == 802 && handle == 0 {
        return Ok(Continue);
    }
    if index == 796 {
        host.quest_tables.remove(&handle).ok_or_else(invalid)?;
        return Ok(Continue);
    }
    let table = host.quest_tables.get_mut(&handle).ok_or_else(invalid)?;
    Ok(match index {
        797 | 799 => {
            table.state[usize::from(index == 799)] = args[1];
            Continue
        }
        798 | 800 => Value(table.state[usize::from(index == 800)]),
        801 => {
            if let Some(record) = table.rows.get_mut(&(args[1] as i32)) {
                let progress = &mut record.progress;
                progress.assignment = UNASSIGNED;
                progress.stamp = table.state[0];
                progress.completed = progress.completed.wrapping_add(1);
            }
            Continue
        }
        802 => {
            table.count_progress(&args, vm)?;
            Continue
        }
        805 => Value(table.position(args[1] as i32, args[2])?),
        807 | 811..=813 => {
            if let Some(slot) = table.counters.get_mut(args[1] as usize) {
                match index {
                    807 => {
                        slot[0] = slot[0].wrapping_add(1);
                        slot[1] = slot[1].wrapping_add(1);
                        slot[2] = slot[2].wrapping_add(u32::from(args[2] != 0));
                        table.attempts = table.attempts.wrapping_add(1);
                    }
                    811 | 812 => {
                        let count = &mut slot[(index - 808) as usize];
                        *count = (*count as i32).wrapping_add(args[2] as i32).max(0) as u32;
                    }
                    813 => slot[5] = slot[5].wrapping_add(1),
                    _ => unreachable!(),
                }
            }
            Continue
        }
        808 => {
            if args[1] == u32::MAX {
                for slot in &mut table.counters {
                    slot[1] = 0;
                }
            } else if let Some(slot) = table.counters.get_mut(args[1] as usize) {
                slot[1] = 0;
            }
            Continue
        }
        817..=819 => Value(table.find_assignment(index, &args)?),
        821 | 822 => {
            for record in table.rows.values_mut() {
                let progress = &mut record.progress;
                progress.assignment = UNASSIGNED;
                if index == 821 {
                    progress.flags = 0;
                    progress.stamp = 0;
                }
            }
            if index == 821 {
                table.counters.fill([0; 6]);
                table.state = [1, 0];
                table.attempts = 0;
            }
            Continue
        }
        823 => {
            let length = table.save_length();
            if args[1] != 0 {
                let bytes = table.save();
                host.raw_buffer_mut(args[1], length)?
                    .copy_from_slice(&bytes);
            }
            Value(length as u32)
        }
        824 => {
            if args[1] == 0 || args[2] < save::HEADER_SIZE as u32 {
                return Ok(Value(0));
            }
            let bytes = host.raw_buffer_rest(args[1])?;
            let Some(saved) = save::SavedProgress::parse(bytes, args[2])? else {
                return Ok(Value(0));
            };
            host.quest_tables.get_mut(&handle).unwrap().restore(saved);
            Value(1)
        }
        826 => Value(
            table
                .rows
                .get(&(args[1] as i32))
                .map_or(u32::MAX, |record| record.progress.elapsed),
        ),
        827 => {
            for record in table.rows.values_mut() {
                let progress = &mut record.progress;
                if progress.assignment != UNASSIGNED {
                    progress.elapsed = progress.elapsed.wrapping_add(1);
                }
            }
            Continue
        }
        828 => {
            table.attempts = 0;
            Continue
        }
        _ => unreachable!(),
    })
}
