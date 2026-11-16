use super::{HostState, Interpreter, NativeReturn, strings, utf8};
use crate::{Error, Result, read};

/// 复制请求的文本；不包含结尾零，UTF-16 保留独立代理单元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardText {
    /// 普通 10 的原始字节，由宿主按源编码解码。
    Bytes(Vec<u8>),
    Utf16(Vec<u16>),
}

/// 普通 10／1039 提交的剪贴板写入请求；宿主负责实际写入和失败处理。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRequest {
    pub task_id: u32,
    pub text: ClipboardText,
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    address: u32,
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let mut source = strings::format(vm, address, 0)?;
    source.truncate(source.iter().position(|&b| b == 0).unwrap_or(source.len()));
    let mut position = 0;
    while position + 1 < source.len() {
        // 原入口按字节替换，也会匹配双字节字符的尾字节。
        if source[position..].starts_with(b"\\n") {
            source[position..position + 2].copy_from_slice(b"\r\n");
            position += 2;
        } else {
            position += 1;
        }
    }
    let text = if index == 10 {
        ClipboardText::Bytes(source)
    } else {
        let units = utf8::decode_units(&source, false)?;
        if units.len() >= 2048 {
            return Err(Error::new(vm.pc, "剪贴板文字超过基线 UTF-16 缓冲区范围"));
        }
        ClipboardText::Utf16(units)
    };
    if host.clipboard_requests.len() >= read::MAX_ITEMS {
        return Err(Error::new(vm.pc, "剪贴板请求数量超过上限"));
    }
    host.clipboard_requests.push(ClipboardRequest {
        task_id: host.task_id,
        text,
    });
    Ok(NativeReturn::Continue)
}
