use super::{Host, Interpreter, MemoryCopyMode, NativeReturn, TaskClock, memory};
use crate::{Error, Result, read};

/// 普通脚本访问其他任务时所需的同步服务。
pub trait TaskServices {
    /// 完成本次同步分派，保留消息第二项的回写；返回前应完成预算续跑。
    fn dispatch_message(&mut self, target: u32, message: &mut [u32; 4]) -> Result<u32>;
    fn enqueue_message(&mut self, target: u32, message: [u32; 4]) -> Result<u32>;
    fn interpreter(&mut self, target: u32) -> Result<&mut Interpreter>;
}

/// 为已有宿主接入普通表的任务消息和跨任务存储访问。
pub struct TaskHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    pub memory_copy_mode: MemoryCopyMode,
    task_id: u32,
}

impl<'a, H: Host, S: TaskServices> TaskHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        inner.set_task_id(0);
        Self {
            inner,
            services,
            memory_copy_mode: MemoryCopyMode::default(),
            task_id: 0,
        }
    }
}

impl<H: Host, S: TaskServices> Host for TaskHost<'_, H, S> {
    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.task_id = id;
        self.inner.set_task_id(id);
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if !matches!(index, 69 | 70 | 72 | 73) {
            return self.inner.call(index, arguments, vm);
        }
        if arguments.len() != 16 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: [u32; 4] = std::array::from_fn(|i| {
            u32::from_le_bytes(arguments[i * 4..i * 4 + 4].try_into().unwrap())
        });
        match index {
            69 | 70 => {
                let mut message = [self.task_id, args[1], args[2], args[3]];
                let value = if index == 70 {
                    self.services.enqueue_message(args[0], message)?
                } else if args[0] != self.task_id {
                    // 正在执行 native 的自身 VM 不处于消息等待状态。
                    self.services.dispatch_message(args[0], &mut message)?
                } else {
                    0
                };
                Ok(NativeReturn::Value(value))
            }
            72 | 73 => {
                let count = read::sized(args[2], vm.pc)?;
                if args[0] == self.task_id {
                    let (destination, source) = if index == 72 {
                        (args[3], args[1])
                    } else {
                        (args[1], args[3])
                    };
                    memory::copy(vm, destination, source, count, self.memory_copy_mode)?;
                } else {
                    let other = self.services.interpreter(args[0])?;
                    if index == 72 {
                        let bytes = other.read(args[1], count)?;
                        vm.write(args[3], bytes)?;
                    } else {
                        let bytes = vm.read(args[3], count)?;
                        other.write(args[1], bytes)?;
                    }
                }
                Ok(NativeReturn::Continue)
            }
            _ => unreachable!(),
        }
    }
}
