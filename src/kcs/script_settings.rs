use super::{HostState, Interpreter, NativeReturn, native, strings};
use crate::{Error, Result, host::storage::fold_key, text};

fn key(vm: &Interpreter, address: u32) -> Result<String> {
    let source = strings::format(vm, address, 0)?;
    let source = source.split(|&byte| byte == 0).next().unwrap_or_default();
    let key = text::cp932_replacing(source);
    if key.encode_utf16().count() >= 256 {
        return Err(Error::new(vm.pc, "脚本设置键超过缓冲区上限"));
    }
    Ok(fold_key(&key))
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let args = native::words(arguments, [2, 4, 3, 3][(index - 192) as usize], vm.pc)?;
    let group = key(vm, args[0])?;
    if index <= 193 {
        let Some(entries) = host.script_settings.group(&group) else {
            return Ok(NativeReturn::Value(0));
        };
        let name = key(vm, args[1])?;
        let Some(bytes) = entries.get(&name) else {
            return Ok(NativeReturn::Value(0));
        };
        if index == 192 {
            return Ok(NativeReturn::Value(u32::from(bytes.len() <= 4)));
        }
        if vm.read(args[2], 1).is_ok() {
            if bytes.len() > args[3] as usize {
                return Ok(NativeReturn::Value(0));
            }
            vm.write(args[2], bytes)?;
        }
        return Ok(NativeReturn::Value(bytes.len() as u32));
    }
    let name = key(vm, args[1])?;
    let bytes = if index == 194 {
        args[2].to_le_bytes().to_vec()
    } else {
        vm.read_string(args[2])?.to_vec()
    };
    let result = host.script_settings.put(group, name, bytes, |settings| {
        match &mut host.configuration_services {
            Some(services) => services.write_script_settings(host.task_id, &settings.to_bytes()),
            None => Ok(true),
        }
    })?;
    Ok(NativeReturn::Value(u32::from(result)))
}
