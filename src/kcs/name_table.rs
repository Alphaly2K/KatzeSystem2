use super::{Interpreter, NativeReturn, ResourceServices, ResourceState, strings};
use crate::{Error, Result, read, resource::ResourceMode, text};

pub(super) struct NameTable(Vec<Entry>);

struct Entry {
    id: u32,
    name: Vec<u8>,
    extra: [Vec<u8>; 2],
    aliases: Vec<Vec<u8>>,
}

impl NameTable {
    fn parse(bytes: &[u8], unicode: bool) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES || bytes.contains(&0) {
            return Err(Error::new(0, "名称表长度无效或包含零字节"));
        }
        let mut entries = Vec::new();
        let mut p = 0;
        while p < bytes.len() {
            let mut row = Vec::new();
            while p < bytes.len() && !matches!(bytes[p], b'\r' | b'\n') {
                row.push(field(bytes, &mut p)?);
                if row.len() > read::MAX_ITEMS {
                    return Err(Error::new(p, "名称表字段数量超过上限"));
                }
            }
            if p < bytes.len() {
                let newline = bytes[p];
                p += 1;
                if newline == b'\r' && bytes.get(p) == Some(&b'\n') {
                    p += 1;
                }
            }
            let Some(id) = row
                .first()
                .filter(|s| s.first().is_some_and(u8::is_ascii_digit))
            else {
                continue;
            };
            let Some(name) = row.get(1).filter(|name| name.len() < 256) else {
                continue;
            };
            let id = text::integer_prefix(id).map_or(0, |(_, id)| id);
            let name = if unicode {
                text::cp932(name)?.into_bytes()
            } else {
                name.clone()
            };
            let extra = std::array::from_fn(|i| row.get(i + 2).cloned().unwrap_or_default());
            if name.len() >= 256 || extra.iter().any(|field| field.len() >= 256) {
                return Err(Error::new(p, "名称表文本超过固定缓冲区上限"));
            }
            let mut aliases = vec![name.clone()];
            aliases.extend(row.into_iter().skip(4));
            if entries.len() == read::MAX_ITEMS {
                return Err(Error::new(p, "名称表记录数量超过上限"));
            }
            entries.push(Entry {
                id,
                name,
                extra,
                aliases,
            });
        }
        Ok(Self(entries))
    }
}

fn field(bytes: &[u8], p: &mut usize) -> Result<Vec<u8>> {
    let quoted = bytes[*p] == b'"';
    *p += usize::from(quoted);
    let mut value = Vec::new();
    while let Some(&byte) = bytes.get(*p) {
        match byte {
            b'"' if bytes.get(*p + 1) == Some(&b'"') => {
                value.push(b'"');
                *p += 2;
            }
            b'"' => {
                *p += 1;
                break;
            }
            b'\t' | b'\r' | b'\n' if !quoted => break,
            b'\r' | b'\n' => {
                *p += 1;
                if bytes
                    .get(*p)
                    .is_some_and(|&next| matches!((byte, next), (b'\r', b'\n') | (b'\n', b'\r')))
                {
                    *p += 1;
                }
            }
            _ => {
                let width = strings::character_width(&bytes[*p..]);
                value.extend_from_slice(&bytes[*p..*p + width]);
                *p += width;
            }
        }
        if value.len() >= 1024 {
            return Err(Error::new(*p, "名称表字段超过读取缓冲区上限"));
        }
    }
    // 结束引号后的余下文本跳过，直到当前字段的分隔符或行末。
    while bytes
        .get(*p)
        .is_some_and(|b| !matches!(b, b'\t' | b'\r' | b'\n'))
    {
        *p += strings::character_width(&bytes[*p..]);
    }
    if bytes.get(*p) == Some(&b'\t') {
        *p += 1;
    }
    Ok(value)
}

impl ResourceState {
    pub(crate) fn name_table_call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
        unicode: bool,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        let count = match index {
            633 | 634 => 1,
            635 | 636 => 2,
            637 => 3,
            638 => 4,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 && !(index != 633 && handle == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if index == 633 {
            let name = strings::format(vm, handle, 0)?;
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let name = &name[..name
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len())];
            let name = services.resolve(b"fes", name)?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            if bytes.is_empty() {
                return Ok(NativeReturn::Value(0));
            }
            let table = NameTable::parse(&bytes, unicode)?;
            let handle = self.allocate_handle(vm.pc)?;
            self.name_tables.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 634 {
            if handle != 0 && self.name_tables.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "名称表句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        let absent = if matches!(index, 635 | 636) {
            u32::MAX
        } else {
            0
        };
        if handle == 0 {
            return Ok(NativeReturn::Value(absent));
        }
        let table = self
            .name_tables
            .get(&handle)
            .ok_or_else(|| Error::new(vm.pc, "名称表句柄无效"))?;
        let argument = read::u32_at(arguments, 4)?;
        if index == 635 {
            let name = strings::format(vm, argument, 0)?;
            let name = &name[..name
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len())];
            let row = table
                .0
                .iter()
                .position(|entry| entry.aliases.iter().any(|alias| alias == name));
            return Ok(NativeReturn::Value(row.map_or(u32::MAX, |row| row as u32)));
        }
        let Some(entry) = table.0.get(argument as usize) else {
            return Ok(NativeReturn::Value(absent));
        };
        if index == 636 {
            return Ok(NativeReturn::Value(entry.id));
        }
        let (value, address) = if index == 637 {
            (&entry.name, read::u32_at(arguments, 8)?)
        } else {
            let mode = read::u32_at(arguments, 8)?;
            let Some(value) = entry.extra.get(mode as usize) else {
                return Ok(NativeReturn::Value(0));
            };
            (value, read::u32_at(arguments, 12)?)
        };
        if index == 637 || vm.read(address, 1).is_ok() {
            let mut bytes = value.clone();
            bytes.push(0);
            vm.write(address, &bytes)?;
        }
        Ok(NativeReturn::Value(1))
    }
}
