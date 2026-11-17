use crate::kcs::{HostState, Interpreter, NativeReturn, WindowAction, WindowRequest};
use crate::{Error, Result, read};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
) -> Result<NativeReturn> {
    if index == 441 {
        let args = super::words(arguments, 3, vm.pc)?;
        let source = crate::kcs::strings::format(vm, args[1], 0)?;
        let folder: Vec<_> = crate::text::cp932_replacing(&source)
            .encode_utf16()
            .collect();
        if folder.len() >= 50 {
            return Err(Error::new(vm.pc, "连续截图前缀超过基线缓冲区范围"));
        }
        host.frame_capture_enabled = args[0];
        host.frame_capture_folder = folder;
        host.frame_capture_index = args[2];
        host.frame_capture_task_id = host.task_id;
        return Ok(NativeReturn::Continue);
    }
    let mode = if index == 628 {
        Some(super::words(arguments, 1, vm.pc)?[0])
    } else {
        super::words(arguments, 0, vm.pc)?;
        None
    };
    if host.window_requests.len() >= read::MAX_ITEMS {
        return Err(Error::new(vm.pc, "KCS 窗口请求数量超过上限"));
    }
    let action = match index {
        628 => {
            host.debug_mode = mode.unwrap();
            WindowAction::RebuildMenu
        }
        845 => WindowAction::ToggleFullscreen,
        846 => WindowAction::Minimize,
        847 => WindowAction::CloseFromMenu,
        _ => unreachable!(),
    };
    host.window_requests.push(WindowRequest {
        task_id: host.task_id,
        action,
    });
    Ok(NativeReturn::Continue)
}
