use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result, read};
use std::collections::BTreeSet;
use std::sync::Arc;

/// 独立 CST 上下文，语言注册在构造时从全局前缀复制。
pub struct CstContext {
    languages: BTreeSet<Vec<u8>>,
    script: Option<Arc<crate::cst::Script>>,
    registrations: Vec<Entry>,
    history: Vec<Entry>,
}

struct Entry {
    name: Vec<u8>,
    value: u32,
}

impl CstContext {
    fn new(prefixes: &[Vec<u8>]) -> Self {
        Self {
            languages: prefixes
                .iter()
                .map(|prefix| {
                    prefix[..prefix.iter().position(|&b| b == 0).unwrap_or(prefix.len())].to_vec()
                })
                .collect(),
            script: None,
            registrations: Vec::new(),
            history: Vec::new(),
        }
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
