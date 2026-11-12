use crate::kcs::{
    HostState, Interpreter, MenuItemRequest, NativeReturn, WindowCloseRequest, hierarchy_query,
    memory, strings,
};
use crate::task::TaskTree;
use crate::{Error, Result, expression, read, text};

pub(super) fn call(
    state: &mut expression::State,
    host: &mut HostState,
    task_tree: Option<&TaskTree>,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if index == 629 {
        if arguments.len() != 12 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut text = Vec::with_capacity(3);
        for bytes in arguments.chunks_exact(4) {
            text.push(strings::format(
                vm,
                u32::from_le_bytes(bytes.try_into().unwrap()),
                0,
            )?);
        }
        host.system_services
            .as_mut()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置系统打开服务"))?
            .open(host.task_id, &text[0], &text[1], &text[2])?;
        return Ok(NativeReturn::Continue);
    }
    if index == 4 {
        return Ok(NativeReturn::Value(host.system_time_ms));
    }
    if index == 68 {
        return Ok(NativeReturn::Value(host.task_id));
    }
    if index == 465 {
        return Ok(NativeReturn::Value(host.debug_mode));
    }
    if index == 65 {
        return Ok(NativeReturn::YieldVoid);
    }
    if index == 284 {
        // 参数个数因版本而异；至少读名称与输出地址。
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        if let Some(out) = args.get(args.len().saturating_sub(1)) {
            vm.write(*out, &0u32.to_le_bytes())?;
        }
        return Ok(NativeReturn::Value(0));
    }
    let count = match index {
        1 | 6 | 9 | 71 | 75 | 207 | 209 | 210 | 212 | 213 | 393 | 1040 => 1,
        8 | 149 | 211 | 392 | 1049 => 2,
        35 | 36 => 3,
        5 | 76 => 0,
        _ => unreachable!(),
    };
    let args = super::words(arguments, count, vm.pc)?;
    Ok(match index {
        392 | 1049 => return crate::kcs::message_text::call(index, &args, vm),

        207 => {
            let identity = std::array::from_fn(|i| if i % 5 == 0 { 1f32.to_bits() } else { 0 });
            if vm.read(args[0], 1).is_ok() {
                let bytes: Vec<_> = identity
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                vm.write(args[0], &bytes)?;
            } else {
                host.implicit_matrix = identity;
            }
            NativeReturn::Continue
        }
        1 | 1040 => return crate::kcs::debug::call(host, index, args[0], vm),
        6 => NativeReturn::Value(args[0]),
        9 => {
            if host.window_close_requests.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "KCS 窗口关闭请求数量超过上限"));
            }
            host.window_close_requests.push(WindowCloseRequest {
                task_id: host.task_id,
                notify: args[0] == 0,
            });
            NativeReturn::Continue
        }
        393 => {
            if host.menu_item_requests.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "KCS 菜单请求数量超过上限"));
            }
            host.menu_item_requests.push(MenuItemRequest {
                task_id: host.task_id,
                command_id: 0x77,
                value: args[0],
            });
            NativeReturn::Continue
        }
        8 | 149 => {
            let values = if index == 8 {
                host.surface_size
            } else {
                host.pointer_position.map(|coordinate| coordinate as u32)
            };
            for (address, value) in args.iter().zip(values) {
                vm.write(*address, &value.to_le_bytes())?;
            }
            NativeReturn::Continue
        }
        5 => NativeReturn::Value(host.task_time.wrapping_sub(host.task_origin)),
        71 => {
            vm.wait_for_message(args[0])?;
            NativeReturn::YieldVoid
        }
        75 | 76 => {
            let tree = task_tree.ok_or_else(|| Error::new(vm.pc, "KCS 宿主未提供任务树"))?;
            hierarchy_query(
                tree,
                host.task_id,
                index,
                args.first().copied().unwrap_or(0),
                vm.pc,
            )?
        }
        35 => {
            memory::copy(
                vm,
                args[0],
                args[1],
                read::sized(args[2], vm.pc)?,
                host.memory_copy_mode,
            )?;
            NativeReturn::Continue
        }
        36 => {
            let count = read::sized(args[2], vm.pc)?;
            vm.read(args[0], count)?;
            vm.write(args[0], &vec![args[1] as u8; count])?;
            NativeReturn::Continue
        }
        209 => NativeReturn::Continue,
        210 => NativeReturn::Value(state.shared.get(args[0] as usize).copied().unwrap_or(0) as u32),
        211 => {
            if let Some(slot) = state.shared.get_mut(args[0] as usize) {
                *slot = args[1] as i32;
            }
            NativeReturn::Continue
        }
        212 => {
            state.current = args[0] as i32;
            NativeReturn::Continue
        }
        213 => {
            let bytes = vm.read_string(args[0])?;
            let source = text::ascii(&bytes)?;
            NativeReturn::Value(expression::eval_parameter(source, state)? as u32)
        }
        _ => unreachable!(),
    })
}
