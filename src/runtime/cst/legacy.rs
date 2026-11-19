use super::CstContext;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn legacy_cst_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let owner = self.host.task_id;
        if index == 219 {
            self.resources
                .legacy_cst
                .insert(owner, CstContext::new(&self.host.languages.prefixes));
            return Ok(NativeReturn::Continue);
        }
        let Some(context) = self.resources.legacy_cst.get_mut(&owner) else {
            return if matches!(index, 225 | 226) {
                Err(Error::new(vm.pc, "任务尚未初始化 CST 登记表"))
            } else {
                Ok(NativeReturn::Value(0))
            };
        };
        if matches!(index, 225 | 226) {
            if index == 225 {
                context.registrations.clear();
            } else {
                context.history.clear();
            }
            return Ok(NativeReturn::Continue);
        }
        let length = match index {
            220 | 227 => 4,
            221 | 223 | 224 => 8,
            222 => 12,
            1035 => 16,
            _ => unreachable!(),
        };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let arg = |i: usize| read::u32_at(arguments, i * 4);
        let formatted = |address, limit| -> Result<Option<Vec<u8>>> {
            if vm.read(address, 1).is_err() {
                return Ok(None);
            }
            let mut name = format_string(vm, address, 0)?;
            name.truncate(
                name.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len()),
            );
            if name.len() >= limit {
                return Err(Error::new(vm.pc, "CST 格式名称超过缓冲区上限"));
            }
            Ok(Some(name))
        };
        let value = match index {
            220 => {
                let Some(name) = formatted(arg(0)?, 260)? else {
                    return Err(Error::new(vm.pc, "CST 文件名称地址无效"));
                };
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(vm.pc, "运行时未配置 CST 资源服务"))?;
                let name = services.resolve(b"scene", &name)?;
                u32::from(context.load(&name, services)?)
            }
            221 => {
                let (input, record) = (arg(0)? as i32, arg(1)? as i32);
                context
                    .script
                    .as_ref()
                    .map_or(u32::MAX, |script| script.record_kind(input, record))
            }
            222 | 1035 => {
                let address = arg(0)?;
                if vm.read(address, 1).is_err() {
                    return Ok(NativeReturn::Value(0));
                }
                let language = if index == 1035 {
                    Some(
                        formatted(arg(3)?, 64)?
                            .ok_or_else(|| Error::new(vm.pc, "CST 查询语言名称地址无效"))?,
                    )
                } else {
                    None
                };
                if let Some(mut text) = context.record_text(
                    arg(1)? as i32,
                    arg(2)? as i32,
                    language.as_deref(),
                    false,
                    false,
                )? {
                    let length = text.len() as u32;
                    text.push(0);
                    vm.write(address, &text)?;
                    length
                } else {
                    vm.read_string(address)?.len() as u32
                }
            }
            223 | 224 | 227 => {
                let Some(name) = formatted(arg(0)?, 128)? else {
                    return Ok(NativeReturn::Value(0));
                };
                if index == 227 {
                    context.registered_value(&name)
                } else {
                    context.register(name, arg(1)?, index == 224, vm.pc)?;
                    1
                }
            }
            _ => unreachable!(),
        };
        Ok(NativeReturn::Value(value))
    }
}
