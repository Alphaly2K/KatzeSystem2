use super::database::table::Table;
use super::{Interpreter, NativeReturn, ResourceServices, ResourceState, native, strings};
use crate::{Error, Result, read, resource::ResourceMode};

type Groups<T> = Vec<(u32, T)>;
type Packs = Vec<(usize, Vec<usize>)>;

#[derive(Default)]
pub(super) struct QuizBank {
    groups: Groups<Groups<Groups<Packs>>>,
    packs: Vec<Vec<u8>>,
    questions: Vec<Question>,
    active: Vec<usize>,
}

struct Question {
    pack: usize,
    id: u32,
    image: u32,
    prompt: Vec<u8>,
    answers: [Vec<u8>; 4],
    counts: [u32; 2],
}

fn take_before_zero(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

// 原路径帮助函数不剥除位于首字节的唯一分隔符。
fn pack_name(path: &[u8]) -> Vec<u8> {
    let path = take_before_zero(path);
    let start = path
        .iter()
        .rposition(|b| matches!(b, b'/' | b'\\'))
        .map_or(0, |i| i + usize::from(i != 0));
    let name = &path[start..];
    let len = name
        .iter()
        .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
        .filter(|&i| name[i] == b'.')
        .unwrap_or(name.len());
    let mut name = name[..len].to_vec();
    let mut i = 0;
    while i < name.len() {
        if matches!(name[i], 0x81..=0x9f | 0xe0..=0xef)
            && name
                .get(i + 1)
                .is_some_and(|b| matches!(b, 0x40..=0x7e | 0x80..=0xfc))
        {
            i += 2;
        } else {
            name[i].make_ascii_uppercase();
            i += 1;
        }
    }
    name
}

fn group<T: Default>(groups: &mut Groups<T>, id: u32) -> &mut T {
    let index = match groups.iter().position(|&(key, _)| key == id) {
        Some(index) => index,
        None => {
            groups.push((id, T::default()));
            groups.len() - 1
        }
    };
    &mut groups[index].1
}

fn row_word(table: &Table, row: usize, name: &[u8]) -> Result<u32> {
    match table.cell(row, name) {
        Some([a, b, c, d]) => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => Err(Error::new(row, "问答题库数值列宽度无效")),
    }
}

fn row_text(table: &Table, row: usize, name: &[u8], capacity: usize) -> Result<Vec<u8>> {
    let cell = table
        .cell(row, name)
        .ok_or_else(|| Error::new(row, "问答题库缺少文字列"))?;
    let text = read::zstr(cell, 0)?;
    if cell.len() > 256 || text.len() >= capacity {
        return Err(Error::new(row, "问答题库文字超过固定字段容量"));
    }
    Ok(text.to_vec())
}

impl QuizBank {
    fn load(&mut self, name: Vec<u8>, table: &Table) -> Result<()> {
        let pack = self.packs.len();
        if pack >= read::MAX_ITEMS || self.questions.len() + table.len() > read::MAX_ITEMS {
            return Err(Error::new(0, "问答题库条目超过上限"));
        }
        // 先校验完整题库，避免留下未登记的不完整题库。
        let mut rows = Vec::new();
        for row in 0..table.len() {
            if table.cell(row, b"G_A").is_none_or(<[u8]>::is_empty) {
                break;
            }
            let groups = [
                row_word(table, row, b"G_A")?,
                row_word(table, row, b"G_B")?,
                row_word(table, row, b"G_C")?,
            ];
            let id = row_word(table, row, b"ID")?;
            rows.push((
                groups,
                Question {
                    pack,
                    id,
                    image: row_word(table, row, b"IMG")?,
                    prompt: row_text(table, row, b"Q", 256)?,
                    answers: [
                        row_text(table, row, b"A1", 64)?,
                        row_text(table, row, b"A2", 64)?,
                        row_text(table, row, b"A3", 64)?,
                        row_text(table, row, b"A4", 64)?,
                    ],
                    counts: [0; 2],
                },
            ));
        }
        for ([a, b, c], question) in rows {
            let packs = group(group(group(&mut self.groups, a), b), c);
            let ids = if packs.last().is_some_and(|&(index, _)| index == pack) {
                &mut packs.last_mut().unwrap().1
            } else {
                packs.push((pack, Vec::new()));
                &mut packs.last_mut().unwrap().1
            };
            if !ids.iter().any(|&i| self.questions[i].id == question.id) {
                ids.push(self.questions.len());
                self.questions.push(question);
            }
        }
        self.packs.push(name);
        Ok(())
    }

    fn rebuild(&mut self) {
        self.active.clear();
        for (_, b) in self.groups.iter().rev() {
            for (_, c) in b.iter().rev() {
                for (_, packs) in c.iter().rev() {
                    for (_, ids) in packs.iter().rev() {
                        self.active.extend(ids.iter().rev().copied());
                    }
                }
            }
        }
    }
}

impl ResourceState {
    pub(crate) fn quiz_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        use NativeReturn::{Continue, Value};
        let count = match index {
            742 => 0,
            743 | 745 | 764 | 769 | 770 => 1,
            744 | 747 | 749 | 766 | 771 | 772 => 2,
            746 | 776 => 3,
            748 => 4,
            _ => unreachable!(),
        };
        let args = native::words(arguments, count, vm.pc)?;
        if index == 742 {
            let handle = self.allocate_handle(vm.pc)?;
            self.quiz_banks.insert(handle, QuizBank::default());
            return Ok(Value(handle));
        }
        let handle = args[0];
        let null = match index {
            743 | 749 | 764 | 766 | 769 | 770 => Continue,
            776 => Value(u32::MAX),
            _ => Value(0),
        };
        if handle == 0 {
            return Ok(null);
        }
        let invalid = || Error::new(vm.pc, "KCS 问答题库句柄无效");
        if index == 743 {
            self.quiz_banks.remove(&handle).ok_or_else(invalid)?;
            return Ok(Continue);
        }
        let quiz = self.quiz_banks.get_mut(&handle).ok_or_else(invalid)?;
        match index {
            744 => {
                let name = strings::format(vm, args[1], 0)?;
                let name = take_before_zero(&name);
                if name.len() >= 256 {
                    return Err(Error::new(vm.pc, "问答资源名称超过固定缓冲区上限"));
                }
                let services =
                    services.ok_or_else(|| Error::new(vm.pc, "问答资源装载需要资源服务"))?;
                let path = services.resolve(b"database", name)?;
                let key = pack_name(&path);
                if quiz.packs.contains(&key) {
                    return Ok(Value(0));
                }
                let Some(bytes) = services.read(&path, ResourceMode::LooseFirst)? else {
                    return Ok(Value(0));
                };
                let Some(table) = Table::parse(&bytes)? else {
                    return Ok(Value(0));
                };
                if key.len() >= 64 {
                    return Err(Error::new(vm.pc, "问答题库名称超过固定字段容量"));
                }
                quiz.load(key, &table)?;
                Ok(Value(1))
            }
            745 => Ok(Value(quiz.questions.len() as u32)),
            764 => {
                quiz.rebuild();
                Ok(Continue)
            }
            769 | 770 => Ok(Continue),
            _ => {
                let output = match index {
                    746 => Some(args[2]),
                    748 => Some(args[3]),
                    _ => None,
                };
                if output.is_some_and(|address| vm.read(address, 1).is_err()) {
                    return Ok(Value(0));
                }
                let Some(&selected) = quiz.active.get(args[1] as usize) else {
                    return Ok(if matches!(index, 747 | 776) {
                        Value(u32::MAX)
                    } else {
                        null
                    });
                };
                let question = &mut quiz.questions[selected];
                Ok(match index {
                    746 => {
                        native::write_string(vm, args[2], &question.prompt, 1)?;
                        Value(1)
                    }
                    747 => Value(question.image),
                    748 => {
                        let answer = question
                            .answers
                            .get((args[2] & 0x03ff_ffff) as usize)
                            .ok_or_else(|| Error::new(vm.pc, "问答题库选项序号越界"))?;
                        native::write_string(vm, args[3], answer, 1)?;
                        Value(1)
                    }
                    749 | 766 => {
                        let counter = &mut question.counts[usize::from(index == 766)];
                        *counter = counter.wrapping_add(1);
                        Continue
                    }
                    771 | 772 => Value(question.counts[(index - 771) as usize]),
                    776 => {
                        native::write_string(vm, args[2], &quiz.packs[question.pack], 1)?;
                        Value(question.id)
                    }
                    _ => unreachable!(),
                })
            }
        }
    }
}
