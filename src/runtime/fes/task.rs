use super::Scene;
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, ObjectHost, TaskLifecycle, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, fes};

/// FES 任务所需的共享表达式状态、媒体更新及启动时的子树调度。
pub trait Services {
    /// 只推进装载期事件；false 表示保留游标等待续跑。
    fn prepare(&mut self, scene: &mut Scene, budget: usize) -> Result<bool>;
    /// 推进当前任务的场景；false 后沿用同一时刻继续本轮。
    fn update(&mut self, scene: &mut Scene, now: u32, budget: usize) -> Result<bool>;
    /// 以父任务 elapsed 更新时钟和子任务，不执行当前任务主体。
    fn start(&mut self, clock: &mut TaskClock) -> Result<()>;
    /// 父任务命令 25 对应的原始正文；没有正文时返回空值。
    fn inherited_context(&self) -> Result<Vec<u8>>;
}

impl<F: ObjectFactory> Services for ObjectHost<'_, F> {
    fn prepare(&mut self, scene: &mut Scene, budget: usize) -> Result<bool> {
        scene.prepare(self.numbers, budget)
    }

    fn update(&mut self, scene: &mut Scene, now: u32, budget: usize) -> Result<bool> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.update_fes_scene(owner, scene, now)
    }

    fn start(&mut self, clock: &mut TaskClock) -> Result<()> {
        let owner = self.host.task_id;
        let parent = self.tree.node(owner).and_then(|node| node.parent);
        let now = match parent {
            Some(parent) => self
                .objects
                .get(&parent)
                .map(|object| object.clock().elapsed())
                .or_else(|| self.running.get(&parent).map(|task| task.clock.elapsed()))
                .ok_or_else(|| Error::new(0, "FES 父任务时钟不可用"))?,
            None => 0,
        };
        clock.update(now);
        self.set_task_clock(clock);
        let mut lifecycle = TaskLifecycle {
            numbers: self.numbers,
            host: self.host,
            resources: self.resources,
            objects: self.objects,
            running: self.running,
            factory: self.factory,
            budget: self.budget,
            limit: self.limit,
        };
        self.tree.update_children(owner, now, 0, &mut lifecycle)
    }

    fn inherited_context(&self) -> Result<Vec<u8>> {
        let Some(parent) = self
            .tree
            .node(self.host.task_id)
            .and_then(|node| node.parent)
        else {
            return Ok(Vec::new());
        };
        if let Some(object) = self.objects.get(&parent) {
            return Ok(if object.control().deletion_requested != 0 {
                Vec::new()
            } else {
                object.creation_context().unwrap_or_default().to_vec()
            });
        }
        self.running
            .get(&parent)
            .map(|task| {
                if task.deleted {
                    Vec::new()
                } else {
                    task.context.clone().unwrap_or_default()
                }
            })
            .ok_or_else(|| Error::new(0, "FES 父任务上下文不可用"))
    }
}

/// 类型 34 的 FES 任务；构造后通过命令 337、338、340、339 显式初始化并启动。
pub struct FesObject {
    id: u32,
    base: BaseObject,
    initialized: bool,
    started: bool,
    scene: Option<Scene>,
    pending: Option<Command>,
    name: Vec<u8>,
    context: Vec<u8>,
    input: u32,
    sound: u32,
    list: Option<u32>,
    clip: crate::graphics::Rect,
    offset: [i32; 2],
    exit_published: bool,
}

impl FesObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            initialized: false,
            started: false,
            scene: None,
            pending: None,
            name: Vec::new(),
            context: Vec::new(),
            input: 0,
            sound: 0,
            list: None,
            clip: crate::graphics::Rect::default(),
            offset: [0; 2],
            exit_published: false,
        }
    }

    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn input_task(&self) -> u32 {
        self.input
    }

    pub fn sound_task(&self) -> u32 {
        self.sound
    }

    fn synchronize(&self, host: &mut dyn Host) {
        host.set_task_id(self.id);
        host.set_task_clock(&self.base.clock);
        host.set_task_context(&self.context);
    }

    fn effects(&mut self, host: &mut dyn Host) {
        if host.take_task_effects(0).delete {
            self.base.control.deletion_requested = 1;
            if let Some(scene) = &mut self.scene {
                scene.set_system_value("_EXITCODE_", -1);
            }
        }
    }

    fn send(
        &self,
        host: &mut dyn Host,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<u32> {
        let mut output = 0;
        if !host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            buffers,
            &mut output,
        )? {
            return Err(Error::new(0, "FES 服务任务不可达"));
        }
        Ok(output)
    }

    fn bind(&mut self) -> Result<()> {
        if let Some(scene) = &mut self.scene {
            let objects = scene.objects_mut()?;
            if let Some(list) = self.list {
                objects.set_image_list(list, self.clip);
            }
            objects.set_canvas_offset(self.offset);
        }
        Ok(())
    }

    fn run_body(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.synchronize(host);
        self.effects(host);
        if self.base.control.deletion_requested != 0 || !self.started {
            return Ok(true);
        }
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| Error::new(0, "FES 尚未装载"))?;
        if !scene.is_pending() {
            let mode = self.send(
                host,
                self.input,
                145,
                [12, 0, 1],
                &mut CommandBuffers::default(),
            )?;
            if mode != 0 {
                return Err(Error::new(0, "FES 输入模式切换尚未接入"));
            }
        }
        let result = services(host)?.update(
            self.scene.as_mut().unwrap(),
            self.base.clock.previous_time,
            budget,
        );
        if !self.exit_published {
            if let Some(code) = self.scene.as_ref().unwrap().exit_request() {
                self.base.control.property = code as u32;
                self.exit_published = true;
            }
        }
        self.effects(host);
        result
    }
}

impl TaskObject for FesObject {
    fn accepts_deferred_delete(&self) -> bool {
        true
    }

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

    fn creation_context(&self) -> Option<&[u8]> {
        Some(&self.context)
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.context = name(context, 32)?.to_vec();
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, _increment: u32, budget: usize) -> Result<bool> {
        if self.pending.is_some() {
            return Err(Error::new(0, "FES 上一次命令尚未完成"));
        }
        self.run_body(host, budget)
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
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        if command.code != 3 && self.pending.is_some_and(|pending| pending != *command) {
            return Err(Error::new(0, "FES 上一次命令尚未完成"));
        }
        self.synchronize(host);
        let b = command.data[1];
        match command.code {
            3 => {
                self.request_delete(host, budget)?;
            }
            25 => {
                if let Some(buffer) = &mut buffers.data1 {
                    if buffer.len() <= self.context.len() {
                        return Err(Error::new(0, "FES 上下文输出容量不足"));
                    }
                    buffer[..self.context.len()].copy_from_slice(&self.context);
                    buffer[self.context.len()] = 0;
                } else if b != 0 {
                    return Err(Error::new(0, "FES 上下文输出缓冲区缺失"));
                }
            }
            26 => {
                if let Some(buffer) = &buffers.data1 {
                    self.set_creation_context(buffer)?;
                } else if b != 0 {
                    return Err(Error::new(0, "FES 上下文输入缓冲区缺失"));
                }
            }
            337 => {
                if self.initialized || b != 0 {
                    return Err(Error::new(
                        0,
                        "FES 初始化需要未初始化对象及运行时隐式共享状态",
                    ));
                }
                self.sound = self.send(host, 0, 35, [0; 3], &mut CommandBuffers::default())?;
                self.input = self.send(host, 0, 34, [0; 3], &mut CommandBuffers::default())?;
                let inherited = services(host)?.inherited_context()?;
                self.set_creation_context(&inherited)?;
                self.initialized = true;
            }
            338 => {
                if self.pending.is_none() {
                    if !self.initialized || self.scene.is_some() {
                        return Err(Error::new(0, "FES 装载需要已初始化且尚未装载的对象"));
                    }
                    let file = name(
                        buffers
                            .data1
                            .as_deref()
                            .ok_or_else(|| Error::new(0, "FES 文件名缓冲区缺失"))?,
                        260,
                    )?;
                    let bytes = host
                        .resource_services()
                        .ok_or_else(|| Error::new(0, "宿主未提供 FES 资源读取"))?
                        .read(file, crate::resource::ResourceMode::LooseFirst)?;
                    let Some(bytes) = bytes else {
                        self.base.control.property = (-2i32) as u32;
                        return Ok(CommandOutcome::finished(false));
                    };
                    let handles = host
                        .handle_allocator()
                        .ok_or_else(|| Error::new(0, "宿主未提供 FES 共享编号源"))?;
                    let mut scene = Scene::new(fes::Program::parse(&bytes)?, handles)?;
                    scene.set_system_value("_EXITCODE_", -1);
                    scene.set_system_value("_SAVEOBJ_", 0);
                    scene.set_system_value("_FRAME_", 0);
                    self.name = file.to_vec();
                    self.scene = Some(scene);
                    self.bind()?;
                    self.pending = Some(*command);
                }
                if !services(host)?.prepare(self.scene.as_mut().unwrap(), budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
                self.pending = None;
            }
            339 => {
                if self.scene.is_none() {
                    return Err(Error::new(0, "FES 启动前必须装载脚本"));
                }
                if self.pending.is_none() {
                    self.started = true;
                    self.pending = Some(*command);
                    services(host)?.start(&mut self.base.clock)?;
                }
                if !self.run_body(host, budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
                self.pending = None;
            }
            340 => {
                if self.scene.as_ref().is_some_and(Scene::is_pending) {
                    return Err(Error::new(0, "FES 本轮推进尚未完成"));
                }
                self.list = Some(b);
                let mut image = [0; 4];
                self.send(
                    host,
                    b,
                    291,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut image),
                    },
                )?;
                let info = host.image(u32::from_le_bytes(image))?.info();
                self.clip = crate::graphics::Rect::from_xywh(
                    0,
                    0,
                    info.size[0] as i32,
                    info.size[1] as i32,
                );
                let mut x = [0; 4];
                let mut y = [0; 4];
                self.send(
                    host,
                    b,
                    290,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: Some(&mut x),
                        data1: Some(&mut y),
                    },
                )?;
                self.offset = [i32::from_le_bytes(x), i32::from_le_bytes(y)];
                self.bind()?;
            }
            9 | 10 | 19 | 27 | 28 | 64 | 65 | 177 | 178 | 341..=358 => {
                return Err(Error::new(0, "FES 专属命令尚未接入"));
            }
            _ => {
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
            }
        }
        Ok(CommandOutcome::finished(false))
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
        if let Some(scene) = &mut self.scene {
            scene.set_system_value("_EXITCODE_", -1);
        }
        self.base.request_delete(host, budget)
    }
}

/// 创建未装载的类型 34 对象，其余类型委托已有工厂。
pub struct FesFactory<F> {
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for FesFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::FES {
            Ok(Box::new(FesObject::new(id)))
        } else {
            self.fallback.create(id, object_kind, name)
        }
    }
}

fn services(host: &mut dyn Host) -> Result<&mut dyn Services> {
    host.fes_services()
        .ok_or_else(|| Error::new(0, "宿主未提供 FES 运行时服务"))
}

fn name(bytes: &[u8], capacity: usize) -> Result<&[u8]> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    if end >= capacity {
        return Err(Error::new(0, "FES 名称或上下文超过容量"));
    }
    Ok(&bytes[..end])
}
