use super::{Host, Interpreter, NativeReturn, TaskClock, strings};
use crate::{Error, Result};

/// 普通脚本使用的同步平台服务。
pub trait SystemServices {
    /// 按系统 open 操作提交请求；平台返回的启动结果不参与 VM 返回值。
    fn open(
        &mut self,
        task_id: u32,
        target: &[u8],
        parameters: &[u8],
        directory: &[u8],
    ) -> Result<()>;
}

/// 为已有宿主接入普通表的系统打开请求。
pub struct SystemHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    task_id: u32,
}

impl<'a, H: Host, S: SystemServices> SystemHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        inner.set_task_id(0);
        Self {
            inner,
            services,
            task_id: 0,
        }
    }
}

impl<H: Host, S: SystemServices> Host for SystemHost<'_, H, S> {
    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.task_id = id;
        self.inner.set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.inner.set_task_context(context);
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if index != 629 {
            return self.inner.call(index, arguments, vm);
        }
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
        self.services
            .open(self.task_id, &text[0], &text[1], &text[2])?;
        Ok(NativeReturn::Continue)
    }
}
