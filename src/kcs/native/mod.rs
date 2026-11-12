use super::{HostState, Interpreter, NativeReturn};
use crate::task::TaskTree;
use crate::{Error, Result, expression};

mod registry;
pub(crate) use registry::{RuntimeHandler, runtime as runtime_native_handler};
mod scalar;
mod settings;
mod storage;
mod system;
mod text;

/// 普通任务表的 native 分派；宿主状态由调用者长期持有。
pub(crate) fn call_native(
    state: &mut expression::State,
    host: &mut HostState,
    task_tree: Option<&TaskTree>,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let index = registry::canonical(index);
    match registry::core(index) {
        registry::CoreHandler::Text => text::call(state, host, index, arguments, vm),
        registry::CoreHandler::Scalar => scalar::call(host, index, arguments, vm),
        registry::CoreHandler::Settings => settings::call(host, index, arguments, vm),
        registry::CoreHandler::Storage => storage::call(state, host, index, arguments, vm),
        registry::CoreHandler::System => system::call(state, host, task_tree, index, arguments, vm),
        registry::CoreHandler::Data => super::data::call(host, index, arguments, vm),
        registry::CoreHandler::MapData => super::map_data::call(host, index, arguments, vm),
        registry::CoreHandler::Sequence => super::sequences::call(host, index, arguments, vm),
        registry::CoreHandler::RecordStack => super::record_stack::call(host, index, arguments, vm),
        registry::CoreHandler::Vector => super::vectors::call(index, arguments, vm),
        registry::CoreHandler::Continue => Ok(NativeReturn::Continue),
        registry::CoreHandler::Zero => Ok(NativeReturn::Value(0)),
        registry::CoreHandler::Unknown => {
            Err(Error::new(vm.pc, format!("未支持的 KCS native {index}")))
        }
    }
}

fn words(arguments: &[u8], count: usize, pc: usize) -> Result<Vec<u32>> {
    if arguments.len() != count * 4 {
        return Err(Error::new(pc, "KCS native 参数字节数不匹配"));
    }
    Ok(arguments
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect())
}

fn write_string(
    vm: &mut Interpreter,
    address: u32,
    bytes: &[u8],
    terminators: usize,
) -> Result<()> {
    let mut output = bytes.to_vec();
    output.resize(output.len() + terminators, 0);
    vm.write(address, &output)
}
