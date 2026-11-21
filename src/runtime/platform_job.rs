use super::{BaseObject, ObjectFactory, ObjectHost, TaskObject, kind};
use crate::host::platform::{PlatformJob, PlatformJobRequest};
use crate::kcs::{Host, Interpreter, NativeReturn, format_string};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

const NOT_STARTED: u32 = (-2i32) as u32;
const NO_PLUGIN: u32 = (-3i32) as u32;

pub(super) struct PlatformJobObject {
    base: BaseObject,
    id: u32,
    kind: u32,
    started: bool,
    result: u32,
    job: Option<Box<dyn PlatformJob>>,
}

impl PlatformJobObject {
    pub(super) fn new(id: u32, kind: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            kind,
            started: false,
            result: NOT_STARTED,
            job: None,
        }
    }

    fn start(&mut self, request: PlatformJobRequest<'_>, host: &mut dyn Host) -> Result<()> {
        if !self.started {
            if let Some(services) = host.platform_job_services() {
                self.job = services.start(self.id, request)?;
            }
            if self.job.is_none() && self.kind == kind::PLATFORM_PLUGIN {
                self.result = NO_PLUGIN;
            }
            self.started = true;
        }
        Ok(())
    }

    fn poll(&mut self) -> bool {
        if let Some(result) = self.job.as_mut().and_then(|job| job.poll()) {
            self.result = result;
            self.job = None;
        }
        self.job.is_none()
    }
}

impl TaskObject for PlatformJobObject {
    fn control(&self) -> &Control {
        self.base.control()
    }

    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }

    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        self.poll();
        Ok(true)
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
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.control().deletion_requested == 0 {
            match (self.kind, command.code) {
                (kind::PLATFORM_PLUGIN, 573) => {
                    let text = buffers
                        .data1
                        .as_deref()
                        .map_or(Ok(&[][..]), |bytes| read::zstr(bytes, 0))?;
                    self.start(PlatformJobRequest::Plugin(text), host)?;
                }
                (kind::IMAGE_IMPORT, 576) => self.start(PlatformJobRequest::ImageImport, host)?,
                (kind::PLATFORM_PLUGIN, 574) | (kind::IMAGE_IMPORT, 577) => {
                    *output = u32::from(self.poll());
                }
                (kind::PLATFORM_PLUGIN, 575) => {
                    self.poll();
                    *output = self.result;
                }
                _ => {
                    return self
                        .base
                        .dispatch_command(kind, command, output, host, budget);
                }
            }
            return Ok(CommandOutcome::finished(false));
        }
        self.base
            .dispatch_command(kind, command, output, host, budget)
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }

    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }

    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.job = None;
        self.base.request_delete(host, budget)
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn platform_job_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let count = if index == 1003 { 8 } else { 4 };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        match index {
            1003 => {
                let address = read::u32_at(arguments, 4)?;
                if vm.read(address, 1).is_ok() {
                    let mut text = format_string(vm, address, 0)?;
                    if let Some(end) = text.iter().position(|&byte| byte == 0) {
                        text.truncate(end);
                    }
                    text.push(0);
                    self.command_target_with_buffers(
                        target,
                        573,
                        [0; 3],
                        &mut CommandBuffers {
                            data1: Some(&mut text),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(NativeReturn::Continue)
            }
            1004 | 1005 => Ok(NativeReturn::Value(self.command_target(
                target,
                if index == 1004 { 574 } else { 575 },
                [0; 3],
            )?)),
            1007 => {
                self.command_target(target, 576, [0; 3])?;
                Ok(NativeReturn::Continue)
            }
            1008 => {
                let target_exists = self
                    .tree
                    .find_reachable(self.host.task_id, target)
                    .is_some_and(|node| node.kind == kind::IMAGE_IMPORT);
                let target_responds = self
                    .objects
                    .get(&target)
                    .is_some_and(|object| object.control().deletion_requested == 0);
                if !target_exists || !target_responds {
                    // The original wrapper returns its task pointer if the receiver skips the write.
                    return Err(Error::new(vm.pc, "图片导入查询没有可用的结果接收者"));
                }
                Ok(NativeReturn::Value(
                    self.command_target(target, 577, [0; 3])?,
                ))
            }
            _ => unreachable!(),
        }
    }
}
