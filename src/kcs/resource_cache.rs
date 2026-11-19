use super::{Interpreter, NativeReturn, ResourceServices, format_string};
use crate::{Error, Result, read, resource::terminated};

pub(crate) fn call(
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<NativeReturn> {
    let count = match index {
        435 => 3,
        437 => 1,
        434 | 436 | 438 => 0,
        _ => unreachable!(),
    };
    if arguments.len() != count * 4 {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
    let cache = services
        .resource_cache()
        .ok_or_else(|| Error::new(vm.pc, "资源服务未提供预读缓存"))?;
    let word = |n: usize| read::u32_at(arguments, n * 4);
    match index {
        434 => cache.clear(),
        435 => {
            let category = format_string(vm, word(1)?, 0)?;
            let name = format_string(vm, word(0)?, 0)?;
            if terminated(&category).len() >= 260 || terminated(&name).len() >= 260 {
                return Err(Error::new(vm.pc, "资源预读参数超过缓冲区容量"));
            }
            let name = services.resolve(terminated(&category), terminated(&name))?;
            cache.enqueue(&name, word(2)? as i32)?;
        }
        436 => {
            cache.start();
            while !cache.advance(services)? {}
        }
        437 => return Ok(NativeReturn::Value(cache.is_ready(word(0)? as i32) as u32)),
        438 => cache.age(),
        _ => unreachable!(),
    }
    Ok(NativeReturn::Continue)
}
