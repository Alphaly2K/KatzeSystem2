use super::{BaseObject, ObjectFactory, ObjectHost, TaskObject};
use crate::kcs::{Host, Interpreter, NativeReturn, OrdinaryTask, SharedMemory, format_string};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

#[derive(Default)]
struct Unloaded {
    base: BaseObject,
    context: Vec<u8>,
    running: i32,
    accumulated_increment: u32,
    messages: Vec<[u32; 4]>,
}

struct Loading {
    command: Command,
    name: Vec<u8>,
    phase: u8,
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn ordinary_load_native(
        &mut self,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if arguments.len() != 12 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        let name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        if name.len() >= 260 {
            return Err(Error::new(vm.pc, "普通脚本名称超过格式缓冲区上限"));
        }
        let services = self
            .resources
            .services
            .as_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置 KCS 资源服务"))?;
        let mut name = services.resolve(b"kcs", name)?;
        name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
        name.push(0);
        let initialize = read::u32_at(arguments, 8)?;
        self.command_target_with_buffers(
            target,
            161,
            [initialize, 0, 0],
            &mut CommandBuffers {
                data1: Some(&mut name),
                ..CommandBuffers::default()
            },
        )?;
        let external = self
            .running
            .get(&self.host.task_id)
            .and_then(|task| task.external.clone())
            .ok_or_else(|| Error::new(vm.pc, "调用任务未提供外部存储描述"))?;
        self.command_target_with_buffers(
            target,
            172,
            [external.len() as u32, 0, 0],
            &mut CommandBuffers {
                external: Some(external),
                ..CommandBuffers::default()
            },
        )?;
        Ok(if initialize == 0 {
            NativeReturn::Continue
        } else {
            NativeReturn::YieldVoid
        })
    }
}

/// 普通 KCS 对象；构造后可通过命令 161 装载，VM 缺失时仍保留任务状态。
pub struct OrdinaryObject {
    pub task: Option<OrdinaryTask>,
    id: u32,
    unloaded: Unloaded,
    shared_numbers: bool,
    loading: Option<Loading>,
    external: SharedMemory,
}

impl OrdinaryObject {
    pub fn new(task: OrdinaryTask) -> Self {
        let external = task.interpreter.external_storage();
        Self {
            id: task.task_id,
            task: Some(task),
            unloaded: Unloaded::default(),
            shared_numbers: true,
            loading: None,
            external,
        }
    }

    pub fn unloaded(id: u32, external: Vec<u8>) -> Result<Self> {
        if external.len() > read::MAX_BYTES {
            return Err(Error::new(0, "普通任务外部存储超过上限"));
        }
        Ok(Self {
            id,
            task: None,
            unloaded: Unloaded::default(),
            shared_numbers: false,
            loading: None,
            external: SharedMemory::new(external)?,
        })
    }

    pub fn running(&self) -> i32 {
        self.task
            .as_ref()
            .map_or(self.unloaded.running, |task| task.running)
    }

    fn require_idle(&self) -> Result<()> {
        if self.loading.is_some() || self.task.as_ref().is_some_and(OrdinaryTask::is_pending) {
            return Err(Error::new(0, "普通任务执行尚未完成"));
        }
        Ok(())
    }

    fn context(&self) -> &[u8] {
        self.task
            .as_ref()
            .map_or(&self.unloaded.context, |task| &task.creation_context)
    }

    fn set_context(&mut self, bytes: &[u8]) -> Result<()> {
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        if end >= 32 {
            return Err(Error::new(0, "普通任务创建上下文超过容量"));
        }
        if let Some(task) = &mut self.task {
            task.set_creation_context(Some(&bytes[..end]))
        } else {
            self.unloaded.context = bytes[..end].to_vec();
            Ok(())
        }
    }

    fn unload(&mut self) {
        if let Some(mut task) = self.task.take() {
            self.unloaded = Unloaded {
                base: BaseObject {
                    control: task.control.clone(),
                    clock: task.clock.clone(),
                },
                context: std::mem::take(&mut task.creation_context),
                running: task.running,
                accumulated_increment: task.accumulated_increment,
                messages: task.queued_messages().to_vec(),
            };
        }
    }

    fn load(
        &mut self,
        command: &Command,
        buffers: &CommandBuffers<'_>,
        mut host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if let Some(loading) = &self.loading {
            if loading.command != *command {
                return Err(Error::new(0, "普通任务装载命令尚未完成"));
            }
        } else {
            self.require_idle()?;
            let name = match buffers.data1.as_deref() {
                Some(name) => name[..name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len())]
                    .to_vec(),
                None => self
                    .interpreter()
                    .ok_or_else(|| Error::new(0, "普通任务装载缺少名称缓冲区"))?
                    .read_string(command.data[1])?
                    .to_vec(),
            };
            self.unload();
            self.loading = Some(Loading {
                command: *command,
                name,
                phase: 0,
            });
        }
        for _ in 0..budget.max(1) {
            match self.loading.as_ref().unwrap().phase {
                0 => {
                    if let Some(program) =
                        host.load_kcs_program(&self.loading.as_ref().unwrap().name)?
                    {
                        let mut task = OrdinaryTask::new(program, Vec::new())?;
                        task.interpreter.bind_external(self.external.clone());
                        task.task_id = self.id;
                        task.control = self.unloaded.base.control.clone();
                        task.clock = self.unloaded.base.clock.clone();
                        task.creation_context = self.unloaded.context.clone();
                        task.accumulated_increment = self.unloaded.accumulated_increment;
                        for message in &self.unloaded.messages {
                            task.enqueue_message(*message)?;
                        }
                        self.unloaded.messages.clear();
                        self.task = Some(task);
                    }
                }
                1 => {
                    let mut available = [0; 4];
                    self.query_parent(host, 177, &mut available, 1)?;
                    if u32::from_le_bytes(available) != 0 {
                        self.shared_numbers = true;
                    }
                    host.set_task_shared_numbers(self.shared_numbers);
                }
                2 => {
                    self.set_context(&[])?;
                    let mut context = [0; 32];
                    self.query_parent(host, 25, &mut context, 0)?;
                    self.set_context(&context)?;
                }
                3 => {
                    if command.data[0] != 0 {
                        if let Some(task) = &mut self.task {
                            if !task.initialize(&mut host, budget)? {
                                return Ok(CommandOutcome::RESUME);
                            }
                        }
                    }
                    self.loading = None;
                    return Ok(CommandOutcome::finished(false));
                }
                _ => unreachable!(),
            }
            self.loading.as_mut().unwrap().phase += 1;
        }
        Ok(CommandOutcome::RESUME)
    }

    fn query_parent(
        &self,
        host: &mut dyn Host,
        code: u32,
        data: &mut [u8],
        propagate: u32,
    ) -> Result<()> {
        let parent = host.parent_task_id()?;
        host.dispatch_task_command(
            Command {
                target: parent,
                sender: self.id,
                code,
                data: [0, 1, propagate],
            },
            &mut CommandBuffers {
                data1: Some(data),
                ..CommandBuffers::default()
            },
            &mut 0,
        )?;
        Ok(())
    }
}

impl TaskObject for OrdinaryObject {
    fn shared_numbers_available(&self) -> Option<bool> {
        Some(self.shared_numbers)
    }
    fn control(&self) -> &Control {
        self.task
            .as_ref()
            .map_or(&self.unloaded.base.control, |task| &task.control)
    }

    fn clock(&self) -> &TaskClock {
        self.task
            .as_ref()
            .map_or(&self.unloaded.base.clock, |task| &task.clock)
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        match &mut self.task {
            Some(task) => (&mut task.control, &mut task.clock),
            None => self.unloaded.base.state(),
        }
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        self.task.as_ref().map(|task| &task.interpreter)
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        self.task.as_mut().map(|task| &mut task.interpreter)
    }

    fn external_storage(&self) -> Option<SharedMemory> {
        Some(self.external.clone())
    }

    fn creation_context(&self) -> Option<&[u8]> {
        Some(self.context())
    }

    fn queued_messages(&self) -> Option<&[[u32; 4]]> {
        Some(self.task.as_ref().map_or(
            self.unloaded.messages.as_slice(),
            OrdinaryTask::queued_messages,
        ))
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.require_idle()?;
        if self.control().deletion_requested == 0 {
            self.set_context(context)?;
        }
        Ok(())
    }

    fn update_body(
        &mut self,
        mut host: &mut dyn Host,
        increment: u32,
        budget: usize,
    ) -> Result<bool> {
        if self.loading.is_some() {
            return Err(Error::new(0, "普通任务装载命令尚未完成"));
        }
        match &mut self.task {
            Some(task) => task.update_body(increment, &mut host, budget),
            None => {
                if self.unloaded.base.control.deletion_requested == 0 && self.unloaded.running != 0
                {
                    self.unloaded.accumulated_increment =
                        self.unloaded.accumulated_increment.wrapping_add(increment);
                    self.unloaded.running = 0;
                }
                Ok(true)
            }
        }
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }

    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        mut host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.control().deletion_requested != 0 && self.loading.is_none() {
            return Ok(CommandOutcome::finished(false));
        }
        match command.code {
            161 => return self.load(command, buffers, host, budget),
            172 => {
                self.require_idle()?;
                if let Some(task) = &mut self.task {
                    let external = if command.data[0] == 0 {
                        SharedMemory::default()
                    } else {
                        buffers
                            .external
                            .as_ref()
                            .ok_or_else(|| Error::new(0, "普通任务外部存储绑定缺少共享视图"))?
                            .slice(0, command.data[0] as usize)?
                    };
                    task.interpreter.bind_external(external.clone());
                    self.external = external;
                }
            }
            25 if buffers.data1.is_some() => {
                self.require_idle()?;
                let context = self.context();
                let destination = buffers.data1.as_deref_mut().unwrap();
                if destination.len() <= context.len() {
                    return Err(Error::new(0, "普通任务创建上下文输出容量不足"));
                }
                destination[..context.len()].copy_from_slice(context);
                destination[context.len()] = 0;
            }
            26 if buffers.data1.is_some() => {
                self.set_creation_context(buffers.data1.as_deref().unwrap())?
            }
            177 => {
                super::save::query_shared_numbers_available(self.shared_numbers, false, buffers)?
            }
            _ => {
                if self.loading.is_some() {
                    return Err(Error::new(0, "普通任务装载命令尚未完成"));
                }
                if let Some(task) = &mut self.task {
                    return task.dispatch_control(kind, command, output, &mut host, budget);
                }
                match command.code {
                    4 | 18 | 22 | 25 | 26 | 30 | 31 | 64 | 162 | 163 => (),
                    9 | 10 => *output = 0,
                    20 | 21 | 65 => *output = 1,
                    6 | 19 | 164..=179 | 336 | 452 => {
                        return Err(Error::new(
                            0,
                            format!("未支持的未装载普通任务命令 {}", command.code),
                        ));
                    }
                    _ => {
                        return self
                            .unloaded
                            .base
                            .dispatch_command(kind, command, output, host, budget);
                    }
                }
            }
        }
        Ok(CommandOutcome::finished(false))
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.require_idle()?;
        if let Some(task) = &mut self.task {
            task.enqueue_message(message)?;
        }
        Ok(())
    }

    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        mut host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        if self.loading.is_some() {
            return Err(Error::new(0, "普通任务装载命令尚未完成"));
        }
        match &mut self.task {
            Some(task) => task.dispatch_message(message, output, &mut host, budget),
            None => {
                *output = 0;
                Ok(true)
            }
        }
    }

    fn request_delete(&mut self, mut host: &mut dyn Host, budget: usize) -> Result<bool> {
        if self.loading.is_some() {
            return Err(Error::new(0, "普通任务装载命令尚未完成"));
        }
        match &mut self.task {
            Some(task) => task.request_delete(&mut host, budget),
            None => self.unloaded.base.request_delete(host, budget),
        }
    }

    fn release(&mut self) -> Result<()> {
        self.require_idle()
    }
}
