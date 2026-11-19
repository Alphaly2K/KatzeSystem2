use super::{BaseObject, ObjectFactory, ObjectHost, TaskObject};
use crate::host::ExportDestination;
use crate::io::SharedSource;
use crate::kcs::{Host, Interpreter, NativeReturn, format_string};
use crate::resource::ResourceMode;
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};

mod bundle;

pub(super) struct ExportObject {
    base: BaseObject,
    id: u32,
    started: bool,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl ExportObject {
    pub(super) fn new(id: u32) -> Self {
        Self {
            base: BaseObject::new(),
            id,
            started: false,
            cancel: Arc::default(),
            worker: None,
        }
    }

    fn start(&mut self, name: &[u8], host: &mut dyn Host) -> Result<()> {
        let Some(services) = host.export_services() else {
            return Ok(());
        };
        let Some(mut destination) = services.select_destination(self.id)? else {
            return Ok(());
        };
        let source = host
            .resource_services()
            .ok_or_else(|| Error::new(0, "运行时未配置导出资源服务"))?
            .open(name, ResourceMode::LooseFirst)?
            .ok_or_else(|| Error::new(0, "找不到导出资源"))?;
        let cancel = Arc::clone(&self.cancel);
        self.worker = Some(
            thread::Builder::new()
                .spawn(move || unpack(source, destination.as_mut(), &cancel))
                .map_err(|err| Error::new(0, format!("导出工作线程启动失败：{err}")))?,
        );
        Ok(())
    }

    fn poll(&mut self, host: &mut dyn Host) -> bool {
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = self
                .worker
                .take()
                .unwrap()
                .join()
                .unwrap_or_else(|_| Err(Error::new(0, "导出工作线程异常结束")));
            if let Some(services) = host.export_services() {
                services.finished(self.id, result);
            }
        }
        self.worker.is_none()
    }
}

fn unpack(
    source: SharedSource,
    destination: &mut dyn ExportDestination,
    cancel: &AtomicBool,
) -> Result<()> {
    let size = u32::try_from(source.len()?).map_err(|_| Error::new(0, "导出包长度溢出"))?;
    let mut bytes = vec![0; read::sized(size, 0)?];
    source.read_exact_at(0, &mut bytes)?;
    bundle::unpack(&bytes, destination, cancel)
}

impl Drop for ExportObject {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.cancel.store(true, Ordering::Relaxed);
            let _ = worker.join();
        }
    }
}

impl TaskObject for ExportObject {
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

    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        self.poll(host);
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
        if self.base.control.deletion_requested == 0 {
            match command.code {
                516 => {
                    if !self.started {
                        let name = match buffers.data1.as_deref() {
                            Some(data) => read::zstr(data, 0)?,
                            None => b"",
                        };
                        self.started = true;
                        if let Err(error) = self.start(name, host) {
                            if let Some(services) = host.export_services() {
                                services.finished(self.id, Err(error));
                            }
                        }
                    }
                    return Ok(CommandOutcome::finished(false));
                }
                517 => {
                    *output = u32::from(self.poll(host));
                    return Ok(CommandOutcome::finished(false));
                }
                _ => {}
            }
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
        self.cancel.store(true, Ordering::Relaxed);
        self.base.request_delete(host, budget)
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn export_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            690 => 8,
            691 => 4,
            692 => 12,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if index == 691 {
            let mut scratch = [0; 4096];
            let output = self.command_target_with_buffers(
                target,
                517,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut scratch),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Value(output));
        }
        let mut names = Vec::with_capacity(2);
        for offset in (4..count).step_by(4) {
            let source = format_string(vm, read::u32_at(arguments, offset)?, 0)?;
            let source = &source[..source.iter().position(|&b| b == 0).unwrap_or(source.len())];
            let mut name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置导出资源服务"))?
                .resolve(b"export", source)?;
            name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "导出资源名称超过基线缓冲区范围"));
            }
            name.push(0);
            names.push(name);
        }
        let (first, remaining) = names.split_first_mut().unwrap();
        self.command_target_with_buffers(
            target,
            516,
            [0; 3],
            &mut CommandBuffers {
                data0: remaining.first_mut().map(Vec::as_mut_slice),
                data1: Some(first),
                ..Default::default()
            },
        )?;
        Ok(NativeReturn::Continue)
    }
}
