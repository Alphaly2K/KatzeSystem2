use super::{Host, Interpreter, MemoryCopyMode, NativeReturn, TaskClock, memory, strings};
use crate::task::{Command, TaskTree};
use crate::{Error, Result, read};

/// 普通脚本访问其他任务时所需的同步服务。
pub trait TaskServices {
    /// 提供服务当前持有的任务树；缺省沿用内层宿主的查询。
    fn task_tree(&self) -> Option<&TaskTree> {
        None
    }

    /// 创建对象、登记父子关系并传递当前任务的创建上下文。
    fn create_task(
        &mut self,
        _sender: u32,
        _parent: u32,
        _kind: u32,
        _name: &[u8],
        _context: &[u8],
    ) -> Result<u32> {
        Err(Error::new(0, "任务服务未提供对象创建"))
    }

    /// 发送删除请求；实际销毁与索引回收由任务更新处理。
    fn request_delete(&mut self, _sender: u32, _target: u32) -> Result<()> {
        Err(Error::new(0, "任务服务未提供删除请求"))
    }

    /// 从 command.sender 同步路由；返回是否可达，output 由目标处理器填写。
    fn dispatch_command(&mut self, _command: Command, _output: &mut u32) -> Result<bool> {
        Err(Error::new(0, "任务服务未提供命令分派"))
    }

    /// 完成本次同步分派，保留消息第二项的回写；返回前应完成预算续跑。
    fn dispatch_message(&mut self, target: u32, message: &mut [u32; 4]) -> Result<u32>;
    fn enqueue_message(&mut self, target: u32, message: [u32; 4]) -> Result<u32>;
    fn interpreter(&mut self, target: u32) -> Result<&mut Interpreter>;
}

/// 为已有宿主接入普通表的任务创建、命令、消息和存储访问。
pub struct TaskHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    pub memory_copy_mode: MemoryCopyMode,
    pub creation_context: Vec<u8>,
    task_id: u32,
}

impl<'a, H: Host, S: TaskServices> TaskHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        inner.set_task_id(0);
        Self {
            inner,
            services,
            memory_copy_mode: MemoryCopyMode::default(),
            creation_context: Vec::new(),
            task_id: 0,
        }
    }
}

impl<H: Host, S: TaskServices> Host for TaskHost<'_, H, S> {
    fn surface_size(&self) -> [u32; 2] {
        self.inner.surface_size()
    }

    fn parent_task_id(&self) -> Result<u32> {
        if let Some(tree) = self.services.task_tree() {
            return tree
                .node(self.task_id)
                .and_then(|node| node.parent)
                .ok_or_else(|| Error::new(0, "当前任务没有可查询的父级"));
        }
        self.inner.parent_task_id()
    }

    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        self.inner.handle_allocator()
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        self.inner.drawables()
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.inner.image(handle)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.inner.image_data(handle)
    }

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        self.inner.images_mut()
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
    ) -> Result<bool> {
        self.inner.update_image_list(list, submit, force)
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        self.services
            .create_task(self.task_id, self.task_id, kind, name, context)
    }

    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.inner.canvas_layout()
    }

    fn resource_services(&mut self) -> Option<&mut dyn super::ResourceServices> {
        self.inner.resource_services()
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.task_id = id;
        self.inner.set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.creation_context.clear();
        self.creation_context.extend_from_slice(context);
        self.inner.set_task_context(context);
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> super::TaskEffects {
        self.inner.take_task_effects(queued_messages)
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if matches!(index, 75 | 76) {
            if let Some(tree) = self.services.task_tree() {
                let count = if index == 75 { 4 } else { 0 };
                if arguments.len() != count {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                let argument = if index == 75 {
                    u32::from_le_bytes(arguments.try_into().unwrap())
                } else {
                    0
                };
                return super::host::hierarchy_query(tree, self.task_id, index, argument, vm.pc);
            }
        }
        let count = match index {
            66 => 3,
            67 | 109 => 1,
            110 => 2,
            69 | 70 | 72 | 73 => 4,
            _ => return self.inner.call(index, arguments, vm),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 4];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        if index == 66 {
            let name = strings::format(vm, args[2], 0)?;
            let end = self
                .creation_context
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(self.creation_context.len());
            let id = self.services.create_task(
                self.task_id,
                args[1],
                args[0],
                &name,
                &self.creation_context[..end],
            )?;
            return Ok(NativeReturn::Value(id));
        }
        if index == 67 {
            self.services.request_delete(self.task_id, args[0])?;
            return Ok(NativeReturn::Continue);
        }
        match index {
            109 | 110 => {
                let mut output = 0;
                let found = self.services.dispatch_command(
                    Command {
                        target: args[0],
                        sender: self.task_id,
                        code: if index == 109 { 96 } else { 97 },
                        data: [0, if index == 110 { args[1] } else { 0 }, 0],
                    },
                    &mut output,
                )?;
                Ok(if index == 109 {
                    NativeReturn::Value(if found { output } else { 0 })
                } else {
                    NativeReturn::Continue
                })
            }
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
