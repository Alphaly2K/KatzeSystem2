use super::database::table::Table;
use super::{
    Interpreter, NativeReturn, ResourceServices, ResourceState, ScriptRandom, native, strings,
};
use crate::{Error, Result, read, resource::ResourceMode};
use std::collections::{BTreeMap, BTreeSet};

mod order;
mod settings;

type Groups<T> = Vec<(u32, T)>;
type Packs = Vec<(usize, Vec<usize>)>;
const STATE_HEADER: usize = 12;
const STATE_RECORD: usize = 88;
const PACK_NAME_CAPACITY: usize = 64;

#[derive(Default)]
pub(super) struct QuizBank {
    groups: Groups<Groups<Groups<Packs>>>,
    packs: Vec<Pack>,
    questions: Vec<Question>,
    active: Vec<usize>,
    settings: BTreeMap<Vec<u8>, settings::Settings>,
    selected: settings::Settings,
    member_flags: [bool; 20],
    dynamic_genres: [Vec<i32>; 3],
}

struct Pack {
    name: Vec<u8>,
    first_by_id: BTreeMap<u32, usize>,
}

struct Question {
    pack: usize,
    id: u32,
    image: u32,
    prompt: Vec<u8>,
    answers: [Vec<u8>; 4],
    progress: Progress,
}

#[derive(Default)]
struct Progress {
    auxiliary: u32,
    counts: [u32; 2],
    priority: u32,
    drawn: u32,
}

impl Progress {
    fn words(&self) -> [u32; 5] {
        [
            self.auxiliary,
            self.counts[1],
            self.counts[0],
            self.priority,
            self.drawn,
        ]
    }

    fn from_words([auxiliary, second, first, priority, drawn]: [u32; 5]) -> Self {
        Self {
            auxiliary,
            counts: [first, second],
            priority,
            drawn,
        }
    }
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
                    progress: Progress::default(),
                },
            ));
        }
        let mut first_by_id = BTreeMap::new();
        for ([a, b, c], question) in rows {
            let packs = group(group(group(&mut self.groups, a), b), c);
            let ids = if packs.last().is_some_and(|&(index, _)| index == pack) {
                &mut packs.last_mut().unwrap().1
            } else {
                packs.push((pack, Vec::new()));
                &mut packs.last_mut().unwrap().1
            };
            if !ids.iter().any(|&i| self.questions[i].id == question.id) {
                first_by_id
                    .entry(question.id)
                    .or_insert(self.questions.len());
                ids.push(self.questions.len());
                self.questions.push(question);
            }
        }
        self.packs.push(Pack { name, first_by_id });
        Ok(())
    }

    fn ordered_ids(groups: &Groups<Groups<Groups<Packs>>>) -> impl Iterator<Item = usize> + '_ {
        groups.iter().rev().flat_map(|(_, groups)| {
            groups.iter().rev().flat_map(|(_, groups)| {
                groups.iter().rev().flat_map(|(_, packs)| {
                    packs
                        .iter()
                        .rev()
                        .flat_map(|(_, ids)| ids.iter().rev().copied())
                })
            })
        })
    }

    fn rebuild(&mut self) {
        self.active.clear();
        let filters: [BTreeSet<i32>; 3] = std::array::from_fn(|level| {
            let mut filter = BTreeSet::new();
            for &genre in &self.selected.genres[level] {
                if genre == settings::DYNAMIC_GENRE {
                    filter.extend(self.dynamic_genres[level].iter().copied());
                } else {
                    filter.insert(genre);
                }
            }
            filter
        });

        for (_, groups) in filtered_groups(&self.groups, &filters[0]) {
            for (_, groups) in filtered_groups(groups, &filters[1]) {
                for (_, packs) in filtered_groups(groups, &filters[2]) {
                    self.active.extend(
                        packs
                            .iter()
                            .rev()
                            .flat_map(|(_, ids)| ids.iter().rev().copied()),
                    );
                }
            }
        }
        for &id in &self.active {
            let progress = &mut self.questions[id].progress;
            progress.priority = progress.counts[0];
            progress.drawn = 0;
        }
    }

    fn draw(&mut self, random: Option<&mut ScriptRandom>, pc: usize) -> Result<u32> {
        order::sort(&mut self.active, |index| {
            let state = &self.questions[index].progress;
            if state.drawn == 0 {
                (false, state.priority)
            } else {
                (true, 0)
            }
        });
        let count = order::candidate_count(&self.active, |index| {
            let state = &self.questions[index].progress;
            (state.priority, state.drawn)
        });
        if count == 0 {
            return Ok(0);
        }
        let random = random.ok_or_else(|| Error::new(pc, "问答抽题需要整数随机流"))?;
        let index = random.next_integer() as usize % count;
        let state = &mut self.questions[self.active[index]].progress;
        state.priority = state.priority.wrapping_add(1);
        state.drawn = 1;
        Ok(index as u32)
    }

    fn export_state(&self, vm: &mut Interpreter, output: u32) -> Result<u32> {
        let length = state_size(self.questions.len(), vm.pc)?;
        if output != 0 {
            let mut bytes = vm.read(output, length)?.to_vec();
            for (destination, value) in bytes[..STATE_HEADER].chunks_exact_mut(4).zip([
                STATE_HEADER as u32,
                STATE_RECORD as u32,
                self.questions.len() as u32,
            ]) {
                destination.copy_from_slice(&value.to_le_bytes());
            }
            for (record, index) in bytes[STATE_HEADER..]
                .chunks_exact_mut(STATE_RECORD)
                .zip(Self::ordered_ids(&self.groups))
            {
                let question = &self.questions[index];
                let name = &self.packs[question.pack].name;
                record[..name.len()].copy_from_slice(name);
                record[name.len()] = 0;
                for (destination, value) in record[PACK_NAME_CAPACITY..]
                    .chunks_exact_mut(4)
                    .zip(std::iter::once(question.id).chain(question.progress.words()))
                {
                    destination.copy_from_slice(&value.to_le_bytes());
                }
            }
            vm.write(output, &bytes)?;
        }
        Ok(length as u32)
    }

    fn import_state(&mut self, vm: &Interpreter, input: u32) -> Result<u32> {
        if input == 0 {
            return Err(Error::new(vm.pc, "问答状态缺少输入缓冲区"));
        }
        let header = vm.read(input, STATE_HEADER)?;
        if read::u32_at(&header, 0)? as usize != STATE_HEADER
            || read::u32_at(&header, 4)? as usize != STATE_RECORD
        {
            return Ok(0);
        }
        let count = read::u32_at(&header, 8)? as usize;
        drop(header);
        let bytes = vm.read(input, state_size(count, vm.pc)?)?;
        for record in bytes[STATE_HEADER..].chunks_exact(STATE_RECORD) {
            let name = read::zstr(&record[..PACK_NAME_CAPACITY], 0)?;
            let Some(pack) = self.packs.iter().find(|pack| pack.name == name) else {
                continue;
            };
            let id = read::u32_at(record, PACK_NAME_CAPACITY)?;
            let Some(&index) = pack.first_by_id.get(&id) else {
                continue;
            };
            let mut words = [0; 5];
            for (word, raw) in words
                .iter_mut()
                .zip(record[PACK_NAME_CAPACITY + 4..].chunks_exact(4))
            {
                *word = u32::from_le_bytes(raw.try_into().unwrap());
            }
            self.questions[index].progress = Progress::from_words(words);
        }
        Ok(1)
    }
}

fn filtered_groups<'a, T>(groups: &'a Groups<T>, filter: &BTreeSet<i32>) -> Vec<&'a (u32, T)> {
    if filter.is_empty() {
        return groups.iter().rev().collect();
    }
    let mut groups: Vec<_> = groups
        .iter()
        .filter(|&&(id, _)| filter.contains(&(id as i32)))
        .collect();
    groups.sort_unstable_by_key(|&&(id, _)| id as i32);
    groups
}

fn state_size(count: usize, pc: usize) -> Result<usize> {
    count
        .checked_mul(STATE_RECORD)
        .and_then(|bytes| bytes.checked_add(STATE_HEADER))
        .filter(|&length| length <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(pc, "问答状态超过大小上限"))
}

impl ResourceState {
    pub(crate) fn quiz_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
        random: Option<&mut ScriptRandom>,
    ) -> Result<NativeReturn> {
        use NativeReturn::{Continue, Value};
        let count = match index {
            742 => 0,
            743
            | 745
            | 752
            | 753
            | 755
            | 756
            | 760..=762
            | 764
            | 765
            | 769
            | 770
            | 773
            | 775
            | 778 => 1,
            744 | 747 | 749..=751 | 754 | 757 | 759 | 766..=768 | 771 | 772 | 774 => 2,
            746 | 758 | 776 | 777 => 3,
            748 => 4,
            763 => 5,
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
            743 | 749 | 757 | 763 | 764 | 766 | 769 | 770 | 773 | 774 | 777 | 778 => Continue,
            755 | 765 | 776 => Value(u32::MAX),
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
                if quiz.packs.iter().any(|pack| pack.name == key) {
                    return Ok(Value(0));
                }
                let Some(bytes) = services.read(&path, ResourceMode::LooseFirst)? else {
                    return Ok(Value(0));
                };
                let Some(table) = Table::parse(&bytes)? else {
                    return Ok(Value(0));
                };
                if key.len() >= PACK_NAME_CAPACITY {
                    return Err(Error::new(vm.pc, "问答题库名称超过固定字段容量"));
                }
                quiz.load(key, &table)?;
                Ok(Value(1))
            }
            745 => Ok(Value(quiz.questions.len() as u32)),
            750 => {
                let name = strings::format(vm, args[1], 0)?;
                let name = take_before_zero(&name);
                if name.len() >= 256 {
                    return Err(Error::new(vm.pc, "问答设置文件名超过固定缓冲区上限"));
                }
                quiz.settings.clear();
                let services =
                    services.ok_or_else(|| Error::new(vm.pc, "问答设置装载需要资源服务"))?;
                let path = services.resolve(b"database", name)?;
                let Some(bytes) = services
                    .read(&path, ResourceMode::LooseFirst)?
                    .filter(|bytes| !bytes.is_empty())
                else {
                    return Ok(Value(0));
                };
                quiz.settings = settings::parse(&bytes)?;
                Ok(Value(1))
            }
            751 => {
                let name = strings::format(vm, args[1], 0)?;
                let Some(settings) = quiz.settings.get(take_before_zero(&name)) else {
                    return Ok(Value(0));
                };
                quiz.selected = settings.clone();
                Ok(Value(1))
            }
            752 => Ok(Value(quiz.selected.count)),
            753 => Ok(Value(quiz.selected.quota)),
            754 | 757 => {
                if vm.read(args[1], 1).is_ok() {
                    let name = if index == 754 {
                        &quiz.selected.background
                    } else {
                        &quiz.selected.rival_image
                    };
                    native::write_string(vm, args[1], name, 1)?;
                    if index == 754 {
                        return Ok(Value(1));
                    }
                }
                Ok(null)
            }
            755 => Ok(Value(quiz.selected.rival)),
            756 => Ok(Value(quiz.selected.rival_type)),
            758 => {
                if args[1..]
                    .iter()
                    .any(|&address| vm.read(address, 1).is_err())
                {
                    return Ok(Value(0));
                }
                let [has_position, x, y] = quiz.selected.rival_position;
                vm.write(args[1], &x.to_le_bytes())?;
                vm.write(args[2], &y.to_le_bytes())?;
                Ok(Value(has_position))
            }
            759 => {
                let id = args[1];
                Ok(Value(u32::from(
                    quiz.selected.partners.contains(&id)
                        || (!quiz.selected.members.contains(&id)
                            && quiz.member_flags.get(id as usize) == Some(&true)),
                )))
            }
            760 => Ok(Value(quiz.selected.music)),
            761 => Ok(Value(quiz.selected.disabled)),
            762 => Ok(Value(quiz.selected.experience)),
            763 => {
                for (&address, value) in args[1..].iter().zip(quiz.selected.results) {
                    vm.write(address, &value.to_le_bytes())?;
                }
                Ok(Continue)
            }
            764 => {
                quiz.rebuild();
                Ok(Continue)
            }
            765 => Ok(Value(quiz.draw(random, vm.pc)?)),
            767 => Ok(Value(quiz.export_state(vm, args[1])?)),
            768 => Ok(Value(quiz.import_state(vm, args[1])?)),
            769 | 770 => Ok(Continue),
            773 => {
                quiz.member_flags.fill(false);
                Ok(Continue)
            }
            774 => {
                if let Some(flag) = quiz.member_flags.get_mut(args[1] as usize) {
                    *flag = true;
                }
                Ok(Continue)
            }
            775 => Ok(Value(u32::from(!quiz.selected.partners.is_empty()))),
            777 => {
                if let Some(ids) = quiz.dynamic_genres.get_mut(args[1] as usize) {
                    if ids.len() >= read::MAX_ITEMS {
                        return Err(Error::new(vm.pc, "问答分组选项超过存储上限"));
                    }
                    ids.push(args[2] as i32);
                }
                Ok(Continue)
            }
            778 => {
                for ids in &mut quiz.dynamic_genres {
                    ids.clear();
                }
                Ok(Continue)
            }
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
                        let counter = &mut question.progress.counts[usize::from(index == 766)];
                        *counter = counter.wrapping_add(1);
                        Continue
                    }
                    771 | 772 => Value(question.progress.counts[(index - 771) as usize]),
                    776 => {
                        native::write_string(vm, args[2], &quiz.packs[question.pack].name, 1)?;
                        Value(question.id)
                    }
                    _ => unreachable!(),
                })
            }
        }
    }
}
