use super::{Interpreter, NativeReturn, ResourceServices, ResourceState, native, strings};
use crate::{Error, Result, read, resource::ResourceMode};

const GROUPS: usize = 20;
const SLOTS: usize = 8;
const BACKGROUND_RECORD: usize = 0x21c;
const GROUP_SIZE: usize = SLOTS * BACKGROUND_RECORD;
const VARIANTS: usize = 16;
const GALLERY_COLUMNS: usize = 4;
const GALLERY_NAME: usize = 32;

pub(super) enum GalleryTable {
    Background(Option<Vec<u8>>),
    Names(Vec<u8>),
}

fn entry_count(group: &[u8]) -> usize {
    group
        .chunks_exact(BACKGROUND_RECORD)
        .filter(|record| record[..4] != [0; 4])
        .count()
}

fn group_count(table: &[u8]) -> usize {
    table
        .chunks_exact(GROUP_SIZE)
        .filter(|group| entry_count(group) != 0)
        .count()
}

fn background_name(
    table: &[u8],
    record: &[u8],
    offset: usize,
    variants: [usize; 2],
) -> Result<Vec<u8>> {
    let mut merged = [0; VARIANTS];
    let first = 0x18 + variants[0] * VARIANTS;
    let second = 0x11c + variants[1] * VARIANTS;
    for start in [first, second] {
        for (byte, &value) in merged.iter_mut().zip(&record[start..start + VARIANTS]) {
            if value != 0 && value != b'0' {
                *byte = value;
            }
        }
    }
    let end = merged
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |i| i + 1);
    let mut name = read::zstr(table, offset + 4)?.to_vec();
    name.push(b',');
    for &byte in &merged[..end] {
        name.push(if byte == 0 { b'0' } else { byte });
        name.push(b',');
    }
    *name.last_mut().unwrap() = 0;
    name.push(0);
    Ok(name)
}

impl ResourceState {
    pub(super) fn gallery_table_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        let words = match index {
            408 => 2,
            409 | 410 | 415 => 3,
            411 => 6,
            416 => 4,
            _ => 1,
        };
        if matches!(index, 408..=410 | 415) && arguments == [0; 4] {
            return Ok(NativeReturn::Value(0));
        }
        let args = native::words(arguments, words, vm.pc)?;
        if matches!(index, 405 | 412) {
            let name = strings::format(vm, args[0], 0)?;
            let name = name.split(|&byte| byte == 0).next().unwrap_or_default();
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "KCS 图鉴表名称超过缓冲区上限"));
            }
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let resolved = services.resolve(b"fes", name)?;
            let file = services.read(&resolved, ResourceMode::LooseFirst)?;
            let table = if index == 405 {
                GalleryTable::Background(file.filter(|bytes| bytes.len() == GROUPS * GROUP_SIZE))
            } else if let Some(file) = file.filter(|bytes| !bytes.is_empty()) {
                if file.len() > read::MAX_BYTES {
                    return Err(Error::new(vm.pc, "KCS CG 列表超过读取上限"));
                }
                GalleryTable::Names(file)
            } else {
                return Ok(NativeReturn::Value(0));
            };
            let handle = self.allocate_handle(vm.pc)?;
            self.gallery_tables.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }
        let handle = args[0];
        let release = matches!(index, 406 | 413);
        if handle == 0 {
            return Ok(if release {
                NativeReturn::Continue
            } else {
                NativeReturn::Value(0)
            });
        }
        let table = self
            .gallery_tables
            .get(&handle)
            .filter(|table| matches!(table, GalleryTable::Background(_)) == (index <= 411))
            .ok_or_else(|| Error::new(vm.pc, "KCS 图鉴表句柄无效"))?;
        if release {
            self.gallery_tables.remove(&handle);
            return Ok(NativeReturn::Continue);
        }
        match table {
            GalleryTable::Background(None) => Ok(NativeReturn::Value(0)),
            GalleryTable::Background(Some(table)) => {
                if index == 407 {
                    return Ok(NativeReturn::Value(group_count(table) as u32));
                }
                let group = args[1] as usize;
                if group >= GROUPS {
                    return Ok(NativeReturn::Value(0));
                }
                let data = &table[group * GROUP_SIZE..(group + 1) * GROUP_SIZE];
                if index == 408 {
                    return Ok(NativeReturn::Value(entry_count(data) as u32));
                }
                let slot = args[2] as usize;
                if slot >= SLOTS {
                    return Ok(NativeReturn::Value(0));
                }
                let offset = slot * BACKGROUND_RECORD;
                let record = &data[offset..offset + BACKGROUND_RECORD];
                let first_count = read::u32_at(record, 0x14)?;
                let second_count = read::u32_at(record, 0x118)?;
                if matches!(index, 409 | 410) {
                    return Ok(NativeReturn::Value(if index == 409 {
                        first_count
                    } else {
                        second_count
                    }));
                }
                let variants = [args[3] as usize, args[4] as usize];
                if group >= group_count(table)
                    || slot >= entry_count(data)
                    || variants.into_iter().any(|variant| variant >= VARIANTS)
                    || args[3] as i32 >= first_count as i32
                    || args[4] as i32 >= second_count as i32
                    || vm.read(args[5], 1).is_err()
                {
                    return Ok(NativeReturn::Value(0));
                }
                let name = background_name(table, record, group * GROUP_SIZE + offset, variants)?;
                vm.write(args[5], &name)?;
                Ok(NativeReturn::Value(1))
            }
            GalleryTable::Names(table) => {
                let count = read::u32_at(table, 0)?;
                if index == 414 {
                    return Ok(NativeReturn::Value(count));
                }
                let row = args[1] as i32;
                let variant = args[2] as usize;
                if row < 0 || row >= count as i32 || variant >= GALLERY_COLUMNS {
                    return Ok(NativeReturn::Value(0));
                }
                let stride = GALLERY_COLUMNS * GALLERY_NAME;
                let Some(at) = (row as usize)
                    .checked_mul(stride)
                    .and_then(|offset| offset.checked_add(4 + variant * GALLERY_NAME))
                    .filter(|&at| at < table.len())
                else {
                    return Err(Error::new(vm.pc, "KCS CG 列表条目超出文件"));
                };
                if table[at] == 0 {
                    return Ok(NativeReturn::Value(0));
                }
                if index == 416 {
                    let name = read::zstr(table, at)?;
                    native::write_string(vm, args[3], name, 1)?;
                }
                Ok(NativeReturn::Value(1))
            }
        }
    }
}
