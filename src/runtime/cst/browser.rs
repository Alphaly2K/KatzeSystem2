use super::{CstContext, ObjectFactory, ObjectHost};
use crate::host::resources::ResourceServices;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::{Error, Result, read};

type Services = Option<Box<dyn ResourceServices>>;

pub(in crate::runtime) struct HistoryBrowser {
    source: u32,
    newest: usize,
    slots: Vec<Slot>,
}

#[derive(Clone, Copy)]
struct Position {
    history: usize,
    input: i32,
    end: i32,
}

impl Position {
    fn at_end(context: &CstContext, index: usize) -> Option<Self> {
        let history = context.history_index(index as u32);
        let end = context.history.get(history)?.value as i32;
        Some(Self {
            history,
            input: end,
            end,
        })
    }
}

#[derive(Default)]
struct Slot {
    position: Option<Position>,
    reader: Option<Box<CstContext>>,
}

impl Slot {
    fn refresh(
        &mut self,
        mut position: Position,
        context: &CstContext,
        services: &mut Services,
    ) -> Result<(Position, bool)> {
        let Some(entry) = Position::at_end(context, position.history) else {
            position.history = u32::MAX as usize;
            return Ok((position, false));
        };
        position.history = entry.history;
        position.end = entry.end;
        let name = context.history[position.history].name();
        // The reader's reserved name stays empty even after a successful resource load.
        let loaded = if name.is_empty() {
            true
        } else {
            let services = services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 CST 资源服务"))?;
            let name = services.resolve(b"scene", name)?;
            self.reader
                .get_or_insert_with(|| Box::new(CstContext::new(&[])))
                .load(&name, services)?
        };
        Ok((position, loaded))
    }

    fn records(&self, input: i32) -> &[crate::cst::Record] {
        self.reader
            .as_ref()
            .and_then(|reader| reader.script())
            .and_then(|script| {
                let range = script.inputs().get(input as usize)?;
                script.records().get(range.clone())
            })
            .unwrap_or_default()
    }

    fn find_text(
        &mut self,
        position: Position,
        context: &CstContext,
        services: &mut Services,
    ) -> Result<(Position, bool)> {
        if position.input >= read::MAX_ITEMS as i32 {
            return Err(Error::new(0, "CST 历史输入段超过浏览上限"));
        }
        let (position, loaded) = self.refresh(position, context, services)?;
        let valid = loaded
            && self
                .records(position.input)
                .iter()
                .any(|r| matches!(r.kind, 0x20 | 0x21));
        Ok((position, valid))
    }

    fn seek(
        &mut self,
        mut position: Position,
        context: &CstContext,
        services: &mut Services,
        newest: usize,
        forward: bool,
    ) -> Result<Option<Position>> {
        if context.history.is_empty() {
            return Ok(None);
        }
        for _ in 0..read::MAX_ITEMS {
            if forward {
                position.input = position.input.saturating_add(1);
                if position.input > position.end
                    || (position.history != newest && position.input == position.end)
                {
                    if position.history >= newest || position.history + 1 >= context.history.len() {
                        return Ok(None);
                    }
                    position = Position::at_end(context, position.history + 1).unwrap();
                    position.input = 0;
                }
            } else {
                position.input = position.input.saturating_sub(1);
                if position.input < 0 {
                    let Some(index) = position.history.checked_sub(1) else {
                        return Ok(None);
                    };
                    let Some(previous) = Position::at_end(context, index) else {
                        return Ok(None);
                    };
                    position = previous;
                }
            }
            let (candidate, valid) = self.find_text(position, context, services)?;
            if valid {
                return Ok(Some(candidate));
            }
            position = candidate;
        }
        Err(Error::new(0, "CST 历史浏览扫描超过上限"))
    }
}

impl HistoryBrowser {
    fn new(
        source: u32,
        count: usize,
        context: &CstContext,
        services: &mut Services,
    ) -> Result<Self> {
        let newest = context.history.len().saturating_sub(1);
        let mut browser = Self {
            source,
            newest,
            slots: (0..count).map(|_| Slot::default()).collect(),
        };
        let Some(position) = Position::at_end(context, newest) else {
            return Ok(browser);
        };
        let first = &mut browser.slots[0];
        let (position, valid) = first.find_text(position, context, services)?;
        first.position = if valid {
            Some(position)
        } else {
            first.seek(position, context, services, newest, false)?
        };
        if count == 1 && first.position.is_some_and(|p| p.input == p.end) {
            browser.step(context, services, false)?;
        }
        for _ in 1..count {
            if !browser.step(context, services, false)? {
                break;
            }
        }
        Ok(browser)
    }

    fn step(
        &mut self,
        context: &CstContext,
        services: &mut Services,
        forward: bool,
    ) -> Result<bool> {
        let count = self.slots.len();
        let (target, origin) = if forward {
            self.slots.rotate_left(1);
            if count == 1 {
                (0, self.slots[0].position)
            } else {
                let previous = self.slots[..count - 1]
                    .iter()
                    .rposition(|slot| slot.position.is_some());
                match previous {
                    Some(index) => (index + 1, self.slots[index].position),
                    None => (count - 1, None),
                }
            }
        } else {
            self.slots.rotate_right(1);
            (0, self.slots[usize::from(count > 1)].position)
        };
        let found = origin
            .map(|position| {
                self.slots[target].seek(position, context, services, self.newest, forward)
            })
            .transpose();
        if let Ok(Some(Some(position))) = found {
            if !(count == 1
                && forward
                && position.history == self.newest
                && position.input >= position.end)
            {
                self.slots[target].position = Some(position);
                return Ok(true);
            }
        }
        if forward {
            self.slots.rotate_right(1);
        } else {
            self.slots.rotate_left(1);
        }
        found.map(|_| false)
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_browser_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let words = match index {
            379 | 381 => 2,
            380 => 1,
            1032 => 5,
            _ => 4,
        };
        if arguments.len() != words * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let arg = |word: usize| read::u32_at(arguments, word * 4);
        let handle = arg(0)?;
        let resources = &mut self.resources;
        if index == 379 {
            let count = arg(1)? as usize;
            if count == 0
                || count > read::MAX_ITEMS
                || resources.cst_browsers.len() >= read::MAX_ITEMS
            {
                return Err(Error::new(vm.pc, "CST 历史浏览槽位或对象数量超出范围"));
            }
            let context = resources
                .cst_contexts
                .get(&handle)
                .ok_or_else(|| Error::new(vm.pc, "CST 历史浏览来源无效"))?;
            let browser = HistoryBrowser::new(handle, count, context, &mut resources.services)?;
            let result = self.host.handle_allocator().allocate()?;
            resources.cst_browsers.insert(result, browser);
            return Ok(NativeReturn::Value(result));
        }
        if index == 380 {
            if handle != 0 && resources.cst_browsers.remove(&handle).is_none() {
                return Err(Error::new(vm.pc, "CST 历史浏览句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        let language = if index == 1032 {
            let mut name = format_string(vm, arg(4)?, 0)?;
            name.truncate(
                name.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len()),
            );
            if name.len() >= 64 {
                return Err(Error::new(vm.pc, "CST 查询语言名超过缓冲区上限"));
            }
            Some(name)
        } else {
            None
        };
        let zero = Ok(NativeReturn::Value(0));
        if handle == 0 {
            return zero;
        }
        let browser = resources
            .cst_browsers
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "CST 历史浏览句柄无效"))?;
        let context = || {
            resources
                .cst_contexts
                .get(&browser.source)
                .ok_or_else(|| Error::new(vm.pc, "CST 历史浏览来源已释放"))
        };
        if index == 381 {
            let context = context()?;
            return Ok(NativeReturn::Value(u32::from(browser.step(
                context,
                &mut resources.services,
                arg(1)? != 0,
            )?)));
        }
        let Some(slot) = browser.slots.get_mut(arg(1)? as usize) else {
            return zero;
        };
        let Some(position) = slot.position else {
            return zero;
        };
        if index == 384 {
            vm.write(arg(2)?, &(position.history as u32).to_le_bytes())?;
            vm.write(arg(3)?, &(position.input as u32).to_le_bytes())?;
            return Ok(NativeReturn::Value(1));
        }
        let (position, loaded) = slot.refresh(position, context()?, &mut resources.services)?;
        slot.position = Some(position);
        if !loaded {
            return zero;
        }
        let body = arg(2)?;
        let name = arg(3)?;
        let has_body = vm.read(body, 1).is_ok();
        let has_name = index != 383 && vm.read(name, 1).is_ok();
        if has_name {
            vm.write(name, &[0])?;
        }
        if has_body {
            vm.write(body, &[0])?;
        }
        let records = slot.records(position.input);
        let mut matched = false;
        let mut commands = 0;
        for (n, record) in records.iter().enumerate() {
            let address = if index == 383 {
                if record.kind != 0x30 {
                    continue;
                }
                if commands != name {
                    commands += 1;
                    continue;
                }
                body
            } else {
                match record.kind {
                    0x20 => body,
                    0x21 => name,
                    _ => continue,
                }
            };
            matched = true;
            if vm.read(address, 1).is_ok() {
                let mut text = slot
                    .reader
                    .as_ref()
                    .unwrap()
                    .record_text(position.input, n as i32, language.as_deref(), false, false)?
                    .unwrap_or_default();
                if text.len() >= 4096 {
                    return Err(Error::new(vm.pc, "CST 历史文本片段超过缓冲区上限"));
                }
                let length = vm.read_string(address)?.len();
                let end = address
                    .checked_add(length as u32)
                    .filter(|&end| (end ^ address) & 0x8000_0000 == 0)
                    .ok_or_else(|| Error::new(vm.pc, "CST 历史文本输出地址越界"))?;
                text.push(0);
                vm.write(end, &text)?;
            }
            if index == 383 {
                break;
            }
        }
        Ok(NativeReturn::Value(u32::from(matched)))
    }
}
