use super::take_before_zero;
use crate::kcs::database::table::Table;
use crate::kcs::{
    Interpreter, NativeReturn, ResourceServices, ResourceState, ScriptRandom, native,
};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::BTreeMap;

const SLOT_COUNT: usize = 19;
const EMPTY: u32 = u32::MAX;
const HIDDEN: u32 = (-10i32) as u32;
const LOCKED: u32 = 1 << 31;

type Slots = [u32; SLOT_COUNT];

#[derive(Default)]
pub(in crate::kcs) struct OptionTables {
    names: BTreeMap<Vec<u8>, u32>,
    rows: BTreeMap<u32, Slots>,
    current: Option<Slots>,
}

impl OptionTables {
    fn load(&mut self, table: &Table) -> Result<()> {
        let mut rows = BTreeMap::new();
        for row in 0..table.len() {
            let id = match table.cell(row, b"TBL") {
                None | Some([]) => break,
                Some([a, b, c, d]) => u32::from_le_bytes([*a, *b, *c, *d]),
                Some(_) => return Err(Error::new(row, "问答选项表编号字段宽度无效")),
            };
            let mut slots = [EMPTY; SLOT_COUNT];
            for (index, slot) in slots.iter_mut().enumerate() {
                let column = [b'P', b'0' + (index / 10) as u8, b'0' + (index % 10) as u8];
                let Some(value) = table.cell(row, &column).filter(|value| !value.is_empty()) else {
                    continue;
                };
                if value.len() > 32 {
                    return Err(Error::new(row, "问答选项名称超过固定字段容量"));
                }
                let name = read::zstr(value, 0)?;
                *slot = self.names.get(name).copied().unwrap_or(EMPTY);
            }
            rows.entry(id).or_insert(slots);
        }
        self.rows = rows;
        Ok(())
    }

    fn lock(&mut self, id: u32, destination: usize, pc: usize) -> Result<()> {
        let Some(slots) = &mut self.current else {
            return Ok(());
        };
        let Some(source) = slots.iter().position(|&value| value & !LOCKED == id) else {
            return Ok(());
        };
        if slots[source] & LOCKED == 0 {
            if destination >= slots.len() {
                return Err(Error::new(pc, "问答选项目标位置越界"));
            }
            slots.swap(source, destination);
            slots[destination] |= LOCKED;
        }
        Ok(())
    }

    fn relocate(
        &mut self,
        source: usize,
        force: bool,
        random: Option<&mut ScriptRandom>,
        pc: usize,
    ) -> Result<()> {
        let slots = self
            .current
            .as_mut()
            .filter(|slots| source < slots.len())
            .ok_or_else(|| Error::new(pc, "问答选项来源位置越界"))?;
        let value = slots[source];
        if value & LOCKED != 0 {
            if force {
                slots[source] = HIDDEN;
            }
            return Ok(());
        }
        let mut empty = [0; SLOT_COUNT - 2];
        let mut count = 0;
        for (index, &slot) in slots.iter().enumerate().skip(2) {
            if slot == EMPTY {
                empty[count] = index;
                count += 1;
            }
        }
        if count != 0 {
            let random = random.ok_or_else(|| Error::new(pc, "移动问答选项需要整数随机流"))?;
            // 脚本后续调用可观察到这一段固定长度的随机消耗。
            for _ in 0..1000 {
                let a = random.next_integer() as usize % count;
                let b = random.next_integer() as usize % count;
                empty.swap(a, b);
            }
            slots[source] = HIDDEN;
            slots[empty[0]] = value;
        } else if force {
            slots[source] = HIDDEN;
        }
        Ok(())
    }
}

impl ResourceState {
    pub(crate) fn quiz_option_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
        services: Option<&mut dyn ResourceServices>,
        random: Option<&mut ScriptRandom>,
    ) -> Result<NativeReturn> {
        use NativeReturn::{Continue, Value};
        let count = match index {
            779 => 0,
            780 | 783 => 1,
            782 | 784 | 829 | 831 | 832 => 2,
            781 | 830 => 3,
            _ => unreachable!(),
        };
        let args = native::words(arguments, count, vm.pc)?;
        if index == 779 {
            let handle = self.allocate_handle(vm.pc)?;
            self.quiz_options.insert(handle, OptionTables::default());
            return Ok(Value(handle));
        }
        let handle = args[0];
        if handle == 0 {
            return Ok(match index {
                782 | 783 => Value(0),
                784 => Value(EMPTY),
                _ => Continue,
            });
        }
        let invalid = || Error::new(vm.pc, "KCS 问答选项表句柄无效");
        if index == 780 {
            self.quiz_options.remove(&handle).ok_or_else(invalid)?;
            return Ok(Continue);
        }
        let options = self.quiz_options.get_mut(&handle).ok_or_else(invalid)?;
        Ok(match index {
            781 | 782 => {
                let name = super::strings::format(vm, args[if index == 781 { 2 } else { 1 }], 0)?;
                let name = take_before_zero(&name);
                if name.len() >= 256 {
                    return Err(Error::new(vm.pc, "问答选项表名称超过固定缓冲区上限"));
                }
                if index == 781 {
                    if options.names.len() == read::MAX_ITEMS && !options.names.contains_key(name) {
                        return Err(Error::new(vm.pc, "问答选项名称数量超过上限"));
                    }
                    options.names.entry(name.to_vec()).or_insert(args[1]);
                    Continue
                } else {
                    let services =
                        services.ok_or_else(|| Error::new(vm.pc, "问答选项表装载需要资源服务"))?;
                    let path = services.resolve(b"database", name)?;
                    let Some(bytes) = services.read(&path, ResourceMode::LooseFirst)? else {
                        return Ok(Value(0));
                    };
                    let Some(table) = Table::parse(&bytes)? else {
                        return Ok(Value(0));
                    };
                    options.load(&table)?;
                    Value(1)
                }
            }
            783 => Value(options.rows.len() as u32),
            784 => Value(
                match options
                    .current
                    .as_ref()
                    .and_then(|row| row.get(args[1] as usize))
                {
                    None | Some(&EMPTY) => EMPTY,
                    Some(&HIDDEN) => HIDDEN,
                    Some(&id) => id & !LOCKED,
                },
            ),
            829 => {
                options.current = options.rows.get(&args[1]).copied();
                Continue
            }
            830 => {
                options.lock(args[1], args[2] as usize, vm.pc)?;
                Continue
            }
            831 | 832 => {
                options.relocate(args[1] as usize, index == 831, random, vm.pc)?;
                Continue
            }
            _ => unreachable!(),
        })
    }
}
