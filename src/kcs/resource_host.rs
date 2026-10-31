use super::{Host, Interpreter, NativeReturn, TaskClock, strings};
use crate::resource::{ResourceMode, XmlDocument};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 普通脚本的同步资源读取服务；None 表示未找到或无法打开。
pub trait ResourceServices {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>>;
}

/// 为已有宿主提供普通表的 XML 资源和独立句柄生命周期。
pub struct ResourceHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    xml: BTreeMap<u32, XmlDocument>,
    next_xml_handle: u32,
}

impl<'a, H: Host, S: ResourceServices> ResourceHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        Self {
            inner,
            services,
            xml: BTreeMap::new(),
            next_xml_handle: 1,
        }
    }
}

impl<H: Host, S: ResourceServices> Host for ResourceHost<'_, H, S> {
    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.inner.set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.inner.set_task_context(context);
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        let count = match index {
            590 | 591 => 1,
            592 | 593 => 3,
            _ => return self.inner.call(index, arguments, vm),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        if index == 590 {
            let name = strings::format(vm, args[0], 0)?;
            let Some(bytes) = self.services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let Ok(xml) = XmlDocument::parse(&bytes) else {
                return Ok(NativeReturn::Value(0));
            };
            if self.xml.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "KCS XML 对象数量超过上限"));
            }
            let handle = self.next_xml_handle;
            self.next_xml_handle = handle
                .checked_add(1)
                .ok_or_else(|| Error::new(vm.pc, "KCS XML 句柄耗尽"))?;
            self.xml.insert(handle, xml);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 591 {
            if args[0] != 0 && self.xml.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS XML 句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            return Ok(if index == 592 {
                NativeReturn::Value(args[1])
            } else {
                NativeReturn::Continue
            });
        }
        let xml = self
            .xml
            .get(&args[0])
            .ok_or_else(|| Error::new(vm.pc, "KCS XML 句柄无效"))?;
        let path = strings::format(vm, args[2], 0)?;
        if index == 592 {
            return Ok(NativeReturn::Value(xml.integer(&path, args[1])?));
        }
        // 原入口以目标自身作为缺失默认值，复制前需要一个完整的零结尾串。
        vm.read_string(args[1])?;
        if let Some(value) = xml.text(&path)? {
            let mut value = value.to_vec();
            value.push(0);
            vm.write(args[1], &value)?;
        }
        Ok(NativeReturn::Continue)
    }
}
