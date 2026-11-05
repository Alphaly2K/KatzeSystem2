use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result, read};
use std::collections::BTreeSet;
use std::sync::Arc;

/// 独立 CST 上下文，语言注册在构造时从全局前缀复制。
pub struct CstContext {
    languages: BTreeSet<Vec<u8>>,
    script: Option<Arc<crate::cst::Script>>,
    localization: Option<Arc<crate::cstl::Table>>,
    position: [i32; 2],
    registrations: Vec<Entry>,
    history: Vec<Entry>,
}

struct Entry {
    name: Vec<u8>,
    value: u32,
}

impl CstContext {
    pub(super) fn new(prefixes: &[Vec<u8>]) -> Self {
        Self {
            languages: prefixes
                .iter()
                .map(|prefix| {
                    prefix[..prefix.iter().position(|&b| b == 0).unwrap_or(prefix.len())].to_vec()
                })
                .collect(),
            script: None,
            localization: None,
            position: [0; 2],
            registrations: Vec::new(),
            history: Vec::new(),
        }
    }

    pub fn position(&self) -> [i32; 2] {
        self.position
    }

    /// 保存场景推进方的当前输入段和记录索引；不执行脚本。
    pub fn set_position(&mut self, input: i32, record: i32) {
        self.position = [input, record];
    }

    pub fn localization(&self) -> Option<&crate::cstl::Table> {
        self.localization.as_deref()
    }

    /// 已登记且与当前本地化表按字节匹配的语言前缀。
    pub fn bound_languages(&self) -> impl Iterator<Item = &[u8]> {
        self.languages
            .iter()
            .filter(|name| {
                self.localization.as_ref().is_some_and(|table| {
                    table
                        .languages
                        .iter()
                        .any(|language| language.as_bytes() == name.as_slice())
                })
            })
            .map(Vec::as_slice)
    }

    pub(super) fn load(
        &mut self,
        name: &[u8],
        services: &mut dyn crate::kcs::ResourceServices,
    ) -> Result<bool> {
        use crate::resource::ResourceMode;
        let Some(data) = services
            .read(name, ResourceMode::LooseFirst)?
            .filter(|data| !data.is_empty())
        else {
            return Ok(false);
        };
        self.localization = None;
        let mut loaded = false;
        if data.starts_with(b"CatScene") {
            self.script = None;
            let compressed = read::u32_at(&data, 8)? as usize;
            let uncompressed = read::u32_at(&data, 12)? as usize;
            let size = if compressed == 0 {
                uncompressed
            } else {
                compressed
            };
            if size.checked_add(16) == Some(data.len()) {
                self.script = Some(Arc::new(crate::cst::Script::parse(&data)?));
                self.position = [0; 2];
                loaded = true;
            }
        }
        let mut local_name = name.to_vec();
        let extension = local_name
            .iter()
            .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
            .filter(|&i| local_name[i] == b'.')
            .unwrap_or(local_name.len());
        local_name.truncate(extension);
        local_name.extend_from_slice(b".cstl");
        if let Some(data) = services
            .read(&local_name, ResourceMode::LooseFirst)?
            .filter(|data| !data.is_empty())
        {
            if data.starts_with(b"CSTL") {
                let (table, _) = crate::cstl::Table::parse_prefix(&data)?;
                self.localization = Some(Arc::new(table));
            }
        }
        Ok(loaded)
    }

    pub(super) fn record_text(
        &self,
        input: i32,
        record: i32,
        language: Option<&[u8]>,
        utf8: bool,
    ) -> Result<Option<Vec<u8>>> {
        let Some(script) = self.script.as_ref() else {
            return Ok(None);
        };
        let input = if input < 0 { self.position[0] } else { input } as usize;
        let record = if record < 0 { self.position[1] } else { record } as usize;
        let Some(range) = script.inputs().get(input) else {
            return Ok(None);
        };
        if record >= range.len() {
            return Ok(None);
        }
        let line = &script.records()[range.start + record];
        if let Some(language) = language.filter(|language| self.languages.contains(*language)) {
            if let Some(table) = self.localization.as_ref() {
                if let Some(column) = table
                    .languages
                    .iter()
                    .position(|name| name.as_bytes() == language)
                {
                    if matches!(line.kind, 0x20 | 0x21) {
                        let message = table.messages.get(input).and_then(|row| row.get(column));
                        let text = if line.kind == 0x21 {
                            message.map(|m| m.name.as_str())
                        } else {
                            let segment = script.records()[range.start..range.start + record]
                                .iter()
                                .filter(|r| r.kind == 0x20)
                                .count();
                            message.and_then(|m| m.content.split('\x1f').nth(segment))
                        };
                        return Ok(Some(text.unwrap_or_default().as_bytes().to_vec()));
                    }
                }
            }
        }
        Ok(Some(if utf8 {
            crate::text::cp932(&line.bytes)?.into_bytes()
        } else {
            line.bytes.clone()
        }))
    }

    /// 按原始字节顺序枚举去重后的语言名，包括已注册的空名称。
    pub fn languages(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.languages.iter().map(Vec::as_slice)
    }

    pub fn script(&self) -> Option<&crate::cst::Script> {
        self.script.as_deref()
    }

    /// 按首次登记顺序枚举名称和值；重复名称保留较大的无符号值。
    pub fn registrations(&self) -> impl ExactSizeIterator<Item = (&[u8], u32)> {
        self.registrations
            .iter()
            .map(|entry| (entry.name.as_slice(), entry.value))
    }

    /// 按追加顺序枚举记录；相邻同名记录合并，非相邻同名记录独立保存。
    pub fn history(&self) -> impl ExactSizeIterator<Item = (&[u8], u32)> {
        self.history
            .iter()
            .map(|entry| (entry.name.as_slice(), entry.value))
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 323 {
            if self.resources.cst_contexts.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "CST 上下文数量超过上限"));
            }
            let context = CstContext::new(&self.host.languages.prefixes);
            let handle = self.host.handle_allocator().allocate()?;
            self.resources.cst_contexts.insert(handle, context);
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if index == 324 {
            if handle != 0 && self.resources.cst_contexts.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "CST 上下文句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if handle == 0 {
            return Ok(match index {
                330 | 331 => NativeReturn::Continue,
                329 => NativeReturn::Value(u32::MAX),
                _ => NativeReturn::Value(0),
            });
        }
        let context = self
            .resources
            .cst_contexts
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?;
        if index == 325 {
            if arguments.len() != 8 {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let mut name = crate::kcs::format_string(vm, read::u32_at(arguments, 4)?, 0)?;
            name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "CST 文件名超过缓冲区上限"));
            }
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置 CST 资源服务"))?;
            let name = services.resolve(b"scene", &name)?;
            return Ok(NativeReturn::Value(u32::from(
                context.load(&name, services)?,
            )));
        }
        if matches!(index, 327 | 1033 | 1037 | 1038) {
            let expected = if matches!(index, 1033 | 1038) { 20 } else { 16 };
            if arguments.len() != expected {
                return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
            }
            let address = read::u32_at(arguments, 4)?;
            if vm.read(address, 1).is_err() {
                return Ok(NativeReturn::Value(0));
            }
            let language = if expected == 20 {
                let mut name = crate::kcs::format_string(vm, read::u32_at(arguments, 16)?, 0)?;
                name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
                if name.len() >= 64 {
                    return Err(Error::new(vm.pc, "CST 查询语言名超过缓冲区上限"));
                }
                Some(name)
            } else {
                None
            };
            let Some(mut text) = context.record_text(
                read::u32_at(arguments, 8)? as i32,
                read::u32_at(arguments, 12)? as i32,
                language.as_deref(),
                matches!(index, 1037 | 1038),
            )?
            else {
                return Ok(NativeReturn::Value(0));
            };
            let length = text.len() as u32;
            text.push(0);
            vm.write(address, &text)?;
            return Ok(NativeReturn::Value(length));
        }
        match index {
            326 => {
                let input = read::u32_at(arguments, 4)? as i32;
                let record = read::u32_at(arguments, 8)? as i32;
                Ok(NativeReturn::Value(
                    context
                        .script
                        .as_ref()
                        .map_or(u32::MAX, |script| script.record_kind(input, record)),
                ))
            }
            330 | 331 => {
                if index == 330 {
                    context.registrations.clear();
                } else {
                    context.history.clear();
                }
                Ok(NativeReturn::Continue)
            }
            328 | 329 | 332 => {
                let mut name = crate::kcs::format_string(vm, read::u32_at(arguments, 4)?, 0)?;
                name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
                if name.len() >= 128 {
                    return Err(Error::new(vm.pc, "CST 格式名称超过缓冲区上限"));
                }
                let end = name
                    .iter()
                    .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
                    .filter(|&i| name[i] == b'.')
                    .unwrap_or(name.len());
                name.truncate(end);
                if index != 332 && name.len() >= 32 {
                    return Err(Error::new(vm.pc, "CST 登记名称超过缓冲区上限"));
                }
                if index == 332 {
                    let value = context
                        .registrations
                        .iter()
                        .find(|entry| entry.name.eq_ignore_ascii_case(&name))
                        .map_or(0, |entry| entry.value);
                    return Ok(NativeReturn::Value(value));
                }
                if name.is_empty() {
                    return Ok(NativeReturn::Value(if index == 328 { 1 } else { u32::MAX }));
                }
                let value = read::u32_at(arguments, 8)?;
                let entries = if index == 328 {
                    &mut context.registrations
                } else {
                    &mut context.history
                };
                let existing = if index == 328 {
                    entries
                        .iter_mut()
                        .find(|entry| entry.name.eq_ignore_ascii_case(&name))
                } else {
                    entries
                        .last_mut()
                        .filter(|entry| entry.name.eq_ignore_ascii_case(&name))
                };
                if let Some(entry) = existing {
                    entry.value = entry.value.max(value);
                } else {
                    if entries.len() == read::MAX_ITEMS {
                        return Err(Error::new(vm.pc, "CST 登记数量超过上限"));
                    }
                    entries.push(Entry { name, value });
                }
                Ok(NativeReturn::Value(if index == 328 {
                    1
                } else {
                    entries.len() as u32 - 1
                }))
            }
            _ => unreachable!(),
        }
    }
}
