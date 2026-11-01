//! 任务树、对象存储与普通脚本宿主的整合。
//!
//! 本模块把任务索引、基类控制、生命周期回调和真实对象存储连接起来，替代此前只能逐项手工搭建的适配层。
//! 对象类型由 [`ObjectFactory`] 创建；提供普通 KCS、基类及 [`crate::graphics::GraphicsFactory`] 的基础二维对象。
//! 其他类型需要对应子系统按 [`TaskObject`] 协议实现，缺失时创建请求返回诊断。
//!
//! 同步消息、删除通知和对象命令在返回前完成预算续跑。普通对象执行期间仍接收删除标记与排队消息，
//! 向执行中普通 VM 同步发送消息返回零；其余需要直接借用该对象的操作返回诊断。

use crate::kcs::{
    Host, HostState, Interpreter, NativeReturn, OrdinaryTask, Program, TaskEffects, call_native,
};
use crate::task::{Command, CommandBuffers, Control, Lifecycle, TaskClock, TaskTree};
use crate::{Error, Result, expression, read};
use std::collections::BTreeMap;

mod graphics;
mod object;
mod resources;

use resources::Resources;

pub use object::{BaseObject, OrdinaryObject, TaskObject};

/// 基线工厂中的对象类型编号。数值来自静态构造与类型追踪，具体工厂仍需逐类接入。
pub mod kind {
    pub const BASE: u32 = 0;
    pub const ROOT: u32 = 1;
    pub const PLANE: u32 = 2;
    pub const BUTTON: u32 = 6;
    pub const INPUT: u32 = 11;
    pub const ORDINARY: u32 = 12;
    pub const SOUND: u32 = 13;
    pub const FES: u32 = 34;
}

/// 默认的单次推进预算；预算耗尽时同步入口继续推进到完成。
pub const DEFAULT_BUDGET: usize = 4096;
/// 连续耗尽推进预算时的同步执行保护阈值。
pub const DEFAULT_LIMIT: usize = 1 << 20;

/// 按对象类型创建对象；未实现的类型返回诊断。
pub trait ObjectFactory {
    /// `id` 为已选定的任务编号，`name` 为命令 66 展开后的名称字节。
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>>;
}

/// 按名称提供已解析的 KCS 程序。
pub trait ProgramLoader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>>;
}

/// 只创建普通 KCS 对象的工厂；其他类型由子系统提供的工厂接入。
pub struct KcsFactory<L> {
    pub programs: L,
    /// 普通任务 VM 的外部存储容量。
    pub external_bytes: usize,
}

impl<L: ProgramLoader> KcsFactory<L> {
    pub fn new(programs: L) -> Self {
        Self {
            programs,
            external_bytes: 0,
        }
    }
}

impl<L: ProgramLoader> ObjectFactory for KcsFactory<L> {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind != kind::ORDINARY {
            return Err(Error::new(0, format!("对象类型 {kind} 需要对应子系统工厂")));
        }
        if self.external_bytes > read::MAX_BYTES {
            return Err(Error::new(0, "普通任务外部存储超过上限"));
        }
        let program = self
            .programs
            .load(name)?
            .ok_or_else(|| Error::new(0, "KCS 程序不存在"))?;
        let mut task = OrdinaryTask::new(program, vec![0; self.external_bytes])?;
        task.task_id = id;
        Ok(Box::new(OrdinaryObject::new(task)))
    }
}

/// 任务树、对象存储和普通宿主状态的组合。
pub struct Runtime<F: ObjectFactory> {
    tree: TaskTree,
    objects: BTreeMap<u32, Box<dyn TaskObject>>,
    running: BTreeMap<u32, RunningTask>,
    numbers: expression::State,
    host: HostState,
    resources: Resources,
    factory: F,
    budget: usize,
    limit: usize,
}

impl<F: ObjectFactory> Runtime<F> {
    /// 以已经创建好的根对象建立任务树；根对象编号用于后续自动编号与查询。
    pub fn new(
        root_id: u32,
        root_kind: u32,
        root: Box<dyn TaskObject>,
        numbers: expression::State,
        factory: F,
    ) -> Self {
        let mut host = HostState::default();
        host.configured_shared_slots = numbers.shared.len() as u32;
        Self {
            tree: TaskTree::new(root_id, root_kind),
            objects: BTreeMap::from([(root_id, root)]),
            running: BTreeMap::new(),
            numbers,
            host,
            resources: Resources::default(),
            factory,
            budget: DEFAULT_BUDGET,
            limit: DEFAULT_LIMIT,
        }
    }

    pub fn tree(&self) -> &TaskTree {
        &self.tree
    }

    pub fn object(&self, id: u32) -> Option<&dyn TaskObject> {
        let object = self.objects.get(&id)?;
        Some(object.as_ref())
    }

    pub fn object_mut(&mut self, id: u32) -> Option<&mut dyn TaskObject> {
        let object = self.objects.get_mut(&id)?;
        Some(object.as_mut())
    }

    /// 以当前画布为纹理输入，向独立目标提交画面；不清空目标或执行设备呈现。
    pub fn render_frame(
        &mut self,
        output: &mut crate::graphics::RenderTarget,
        projection: &crate::graphics::Projection,
        parent: [f32; 16],
        filter: crate::graphics::TextureFilter,
    ) -> Result<bool> {
        let canvas = self
            .resources
            .drawing
            .target
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        self.resources
            .drawables
            .render_frame(output, canvas, projection, parent, filter)
    }

    /// 配置同步资源读取与类别名称补全；已有资源对象继续保留。
    pub fn set_resources(&mut self, services: impl crate::kcs::ResourceServices + 'static) {
        self.resources.services = Some(Box::new(services));
    }

    pub fn images(&self) -> &crate::graphics::Images {
        &self.resources.images
    }

    /// 365 装载的原始图像字节，存活至 366 显式释放或运行时销毁。
    pub fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.resources.raw.image_data(handle)
    }

    pub fn drawing(&self) -> &crate::graphics::DrawingContext {
        &self.resources.drawing
    }

    pub fn drawing_mut(&mut self) -> &mut crate::graphics::DrawingContext {
        &mut self.resources.drawing
    }

    pub fn drawables(&self) -> &crate::graphics::Drawables {
        &self.resources.drawables
    }

    pub fn drawables_mut(&mut self) -> &mut crate::graphics::Drawables {
        &mut self.resources.drawables
    }

    /// 更新完成后显式绘制基础纯色及渐变图元；帧清空和平台呈现由调用者驱动。
    pub fn render_triangles(
        &mut self,
        projection: &crate::graphics::Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.resources
            .drawing
            .render_triangles(&mut self.resources.drawables, projection, parent)
    }

    pub fn host(&self) -> &HostState {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HostState {
        &mut self.host
    }

    pub fn numbers(&self) -> &expression::State {
        &self.numbers
    }

    pub fn numbers_mut(&mut self) -> &mut expression::State {
        &mut self.numbers
    }

    pub fn factory_mut(&mut self) -> &mut F {
        &mut self.factory
    }

    /// 设置单次推进预算和同步续跑保护阈值；零预算按一处理。
    pub fn set_budget(&mut self, budget: usize, limit: usize) {
        self.budget = budget.max(1);
        self.limit = limit;
    }

    /// 完成一轮树更新：先更新时钟，再按编号推进子任务，最后执行主体。
    pub fn update(&mut self, now: u32, increment: u32) -> Result<bool> {
        let mut lifecycle = TaskLifecycle {
            numbers: &mut self.numbers,
            host: &mut self.host,
            resources: &mut self.resources,
            objects: &mut self.objects,
            running: &mut self.running,
            factory: &mut self.factory,
            budget: self.budget,
            limit: self.limit,
        };
        self.tree.update(now, increment, &mut lifecycle)
    }

    /// 同步销毁子树；根任务关闭后树不再接受更新。
    pub fn destroy(&mut self, id: u32) -> Result<()> {
        let mut lifecycle = TaskLifecycle {
            numbers: &mut self.numbers,
            host: &mut self.host,
            resources: &mut self.resources,
            objects: &mut self.objects,
            running: &mut self.running,
            factory: &mut self.factory,
            budget: self.budget,
            limit: self.limit,
        };
        self.tree.destroy(id, &mut lifecycle)
    }

    /// 创建对象、登记父子关系并传递原始创建上下文；对应命令 66 的三步顺序。
    ///
    /// 普通 KCS 程序在工厂返回前装载。基线中装载发生在构造还是首次更新尚未核实，
    /// 需要其他时点时由调用者提供相应的工厂实现。
    pub fn create(&mut self, parent: u32, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        create_object(
            &mut self.tree,
            &mut self.objects,
            &mut self.factory,
            parent,
            kind,
            name,
            context,
        )
    }

    /// 由外层调度器按 `origin` 路由任务命令；目标不可达时返回零。
    ///
    /// `command.sender` 保持调用者给出的值，不因路由起点而改写。
    pub fn dispatch_command(&mut self, origin: u32, command: Command) -> Result<u32> {
        route(self.store(), origin, command)
    }

    /// 按同一路由分派带显式输入／输出缓冲区的对象命令。
    pub fn dispatch_command_with_buffers(
        &mut self,
        origin: u32,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<u32> {
        route_with_buffers(self.store(), origin, command, buffers)
    }

    fn store(&mut self) -> Store<'_, F> {
        Store {
            numbers: &mut self.numbers,
            host: &mut self.host,
            resources: &mut self.resources,
            tree: &mut self.tree,
            objects: &mut self.objects,
            running: &mut self.running,
            factory: &mut self.factory,
            budget: self.budget,
            limit: self.limit,
        }
    }

    /// 由外层调度器同步投递任务消息；返回目标 VM 的活动结果。
    pub fn send_message(&mut self, sender: u32, target: u32, message: [u32; 4]) -> Result<u32> {
        let mut message = message;
        self.host_context(sender)
            .message_target(target, &mut message)
    }

    /// 外层入口使用的宿主上下文；当前任务编号不来自脚本，由调用者提供。
    fn host_context(&mut self, current: u32) -> ObjectHost<'_, F> {
        let host = ObjectHost {
            numbers: &mut self.numbers,
            host: &mut self.host,
            resources: &mut self.resources,
            tree: &mut self.tree,
            objects: &mut self.objects,
            running: &mut self.running,
            factory: &mut self.factory,
            context: Vec::new(),
            budget: self.budget,
            limit: self.limit,
        };
        host.host.task_id = current;
        host
    }
}

/// 选取最小可用编号、创建对象、传递上下文并登记父子关系。
///
/// 命令 66 在返回前完成创建、登记和上下文传递；这里先完成上下文，任何失败都不会留下
/// 已占用但无对象的编号。
fn create_object<F: ObjectFactory>(
    tree: &mut TaskTree,
    objects: &mut BTreeMap<u32, Box<dyn TaskObject>>,
    factory: &mut F,
    parent: u32,
    kind: u32,
    name: &[u8],
    context: &[u8],
) -> Result<u32> {
    if tree.node(parent).is_none() {
        return Err(Error::new(0, "任务父级不存在"));
    }
    let id = tree.next_id();
    let mut object = factory.create(id, kind, name)?;
    object.set_creation_context(context)?;
    tree.insert(parent, id, kind)?;
    objects.insert(id, object);
    Ok(id)
}

/// 对象主体或命令的一次执行；由 [`run_nested`] 在对象已从存储中取出后派发。
enum Execution<'a, 'b> {
    Body {
        increment: u32,
    },
    Message {
        message: &'a mut [u32; 4],
        output: &'a mut u32,
    },
    Command {
        kind: u32,
        command: &'a Command,
        buffers: &'a mut CommandBuffers<'b>,
        output: &'a mut u32,
        /// 预算续跑结束后的树传播结果。
        propagate: &'a mut bool,
    },
    Delete,
    MarkDestroying,
}

struct RunningTask {
    deleted: bool,
    remaining_messages: Option<usize>,
    effects: TaskEffects,
}

/// 一次嵌套执行需要的可变部分。
struct Store<'a, F: ObjectFactory> {
    numbers: &'a mut expression::State,
    host: &'a mut HostState,
    resources: &'a mut Resources,
    tree: &'a mut TaskTree,
    objects: &'a mut BTreeMap<u32, Box<dyn TaskObject>>,
    running: &'a mut BTreeMap<u32, RunningTask>,
    factory: &'a mut F,
    budget: usize,
    limit: usize,
}

/// 取出目标对象执行一次，完成后放回并恢复宿主记录的当前任务状态。
fn run_nested<F: ObjectFactory>(
    store: Store<'_, F>,
    target: u32,
    execution: Execution<'_, '_>,
) -> Result<()> {
    let Store {
        numbers,
        host,
        resources,
        tree,
        objects,
        running,
        factory,
        budget,
        limit,
    } = store;
    let mut object = objects
        .remove(&target)
        .ok_or_else(|| Error::new(0, "任务对象不存在或正在执行"))?;
    running.insert(
        target,
        RunningTask {
            deleted: object.control().deletion_requested != 0,
            remaining_messages: object
                .queued_messages()
                .map(|messages| read::MAX_ITEMS.saturating_sub(messages.len())),
            effects: TaskEffects::default(),
        },
    );
    let saved = (host.task_id, host.task_time, host.task_origin);
    let result = {
        let mut nested = ObjectHost {
            numbers: &mut *numbers,
            host: &mut *host,
            resources: &mut *resources,
            tree: &mut *tree,
            objects: &mut *objects,
            running: &mut *running,
            factory: &mut *factory,
            context: Vec::new(),
            budget,
            limit,
        };
        nested.host.task_id = target;
        match execution {
            Execution::Body { increment } => drain(budget, limit, || {
                object.update_body(&mut nested, increment, budget)
            }),
            Execution::Message { message, output } => drain(budget, limit, || {
                object.dispatch_message(message, output, &mut nested, budget)
            }),
            Execution::Command {
                kind,
                command,
                buffers,
                output,
                propagate,
            } => drain(budget, limit, || {
                let outcome = object.dispatch_command_with_buffers(
                    kind,
                    command,
                    buffers,
                    output,
                    &mut nested,
                    budget,
                )?;
                *propagate = outcome.propagate;
                Ok(outcome.done)
            }),
            Execution::Delete => {
                drain(budget, limit, || object.request_delete(&mut nested, budget))
            }
            Execution::MarkDestroying => drain(budget, limit, || {
                object.mark_destroying(&mut nested, budget)
            }),
        }
    };
    running.remove(&target);
    objects.insert(target, object);
    host.task_id = saved.0;
    host.task_time = saved.1;
    host.task_origin = saved.2;
    result
}

fn drain(budget: usize, limit: usize, mut step: impl FnMut() -> Result<bool>) -> Result<()> {
    let budget = budget.max(1);
    let mut used = 0;
    loop {
        if step()? {
            return Ok(());
        }
        used += budget;
        if used > limit {
            return Err(Error::new(0, "同步执行未在指令预算内完成"));
        }
    }
}

/// 按树契约分派一条命令：可达性、目标改写与 `data[2]` 传播由 [`TaskTree::dispatch`] 负责。
fn route<F: ObjectFactory>(store: Store<'_, F>, origin: u32, command: Command) -> Result<u32> {
    route_with_buffers(store, origin, command, &mut CommandBuffers::default())
}

fn route_with_buffers<F: ObjectFactory>(
    store: Store<'_, F>,
    origin: u32,
    command: Command,
    buffers: &mut CommandBuffers<'_>,
) -> Result<u32> {
    let mut output = 0;
    let found = route_command(store, origin, command, buffers, &mut output)?;
    Ok(if found { output } else { 0 })
}

fn route_command<F: ObjectFactory>(
    store: Store<'_, F>,
    origin: u32,
    command: Command,
    buffers: &mut CommandBuffers<'_>,
    output: &mut u32,
) -> Result<bool> {
    let Store {
        numbers,
        host,
        resources,
        tree,
        objects,
        running,
        factory,
        budget,
        limit,
    } = store;
    tree.dispatch(origin, command, output, |tree, command, output| {
        let kind = tree.node(command.target).map_or(0, |node| node.kind);
        let mut propagate = false;
        run_nested(
            Store {
                numbers: &mut *numbers,
                host: &mut *host,
                resources: &mut *resources,
                tree,
                objects: &mut *objects,
                running: &mut *running,
                factory: &mut *factory,
                budget,
                limit,
            },
            command.target,
            Execution::Command {
                kind,
                command,
                buffers,
                output,
                propagate: &mut propagate,
            },
        )?;
        Ok(propagate)
    })
}

/// 执行中对象的宿主：普通表 native 委托 [`call_native`]，任务 native 使用对象存储。
struct ObjectHost<'a, F: ObjectFactory> {
    numbers: &'a mut expression::State,
    host: &'a mut HostState,
    resources: &'a mut Resources,
    tree: &'a mut TaskTree,
    objects: &'a mut BTreeMap<u32, Box<dyn TaskObject>>,
    running: &'a mut BTreeMap<u32, RunningTask>,
    factory: &'a mut F,
    context: Vec<u8>,
    budget: usize,
    limit: usize,
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    fn store(&mut self) -> Store<'_, F> {
        Store {
            numbers: &mut *self.numbers,
            host: &mut *self.host,
            resources: &mut *self.resources,
            tree: &mut *self.tree,
            objects: &mut *self.objects,
            running: &mut *self.running,
            factory: &mut *self.factory,
            budget: self.budget,
            limit: self.limit,
        }
    }

    fn require_reachable(&self, target: u32) -> bool {
        self.tree
            .find_reachable(self.host.task_id, target)
            .is_some()
    }

    fn message_target(&mut self, target: u32, message: &mut [u32; 4]) -> Result<u32> {
        if !self.require_reachable(target) {
            return Err(Error::new(0, "任务消息目标不存在"));
        }
        if self
            .running
            .get(&target)
            .is_some_and(|task| task.remaining_messages.is_some())
        {
            return Ok(0);
        }
        let mut output = 0;
        run_nested(
            self.store(),
            target,
            Execution::Message {
                message,
                output: &mut output,
            },
        )?;
        Ok(output)
    }

    fn command_target(&mut self, target: u32, code: u32, data: [u32; 3]) -> Result<u32> {
        self.command_target_with_buffers(target, code, data, &mut CommandBuffers::default())
    }

    fn command_target_with_buffers(
        &mut self,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<u32> {
        let sender = self.host.task_id;
        route_with_buffers(
            self.store(),
            sender,
            Command {
                target,
                sender,
                code,
                data,
            },
            buffers,
        )
    }

    fn task_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            66 => 12,
            67 | 75 | 109 => 4,
            110 => 8,
            69 | 70 | 72 | 73 => 16,
            76 => 0,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 4];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        match index {
            75 | 76 => crate::kcs::hierarchy_query(
                self.tree,
                self.host.task_id,
                index,
                args.first().copied().unwrap_or(0),
                vm.pc,
            ),
            66 => {
                // 命令先路由到父任务；不可达的父级不能接受创建请求。
                if !self.require_reachable(args[1]) {
                    return Err(Error::new(vm.pc, "任务创建父级不可达"));
                }
                let name = crate::kcs::format_string(vm, args[2], 0)?;
                let end = self
                    .context
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(self.context.len());
                let id = create_object(
                    &mut *self.tree,
                    &mut *self.objects,
                    &mut *self.factory,
                    args[1],
                    args[0],
                    &name,
                    &self.context[..end],
                )?;
                Ok(NativeReturn::Value(id))
            }
            67 => {
                if !self.require_reachable(args[0]) {
                    return Err(Error::new(vm.pc, "任务删除目标不存在"));
                }
                if let Some(task) = self.running.get_mut(&args[0]) {
                    if task.remaining_messages.is_none() {
                        return Err(Error::new(vm.pc, "执行中对象不支持删除请求"));
                    }
                    if !task.deleted {
                        task.deleted = true;
                        task.effects.delete = true;
                    }
                } else {
                    run_nested(self.store(), args[0], Execution::Delete)?;
                }
                Ok(NativeReturn::Continue)
            }
            69 | 70 => {
                let mut message = [self.host.task_id, args[1], args[2], args[3]];
                if index == 70 {
                    if !self.require_reachable(args[0]) {
                        return Err(Error::new(vm.pc, "任务消息目标不存在"));
                    }
                    if let Some(task) = self.running.get_mut(&args[0]) {
                        let remaining = task
                            .remaining_messages
                            .as_mut()
                            .ok_or_else(|| Error::new(vm.pc, "执行中对象不支持排队消息"))?;
                        if task.deleted {
                            return Err(Error::new(vm.pc, "已删除任务的排队返回值未初始化"));
                        }
                        if *remaining == 0 {
                            return Err(Error::new(vm.pc, "普通任务消息数量超过上限"));
                        }
                        *remaining -= 1;
                        task.effects.messages.push(message);
                    } else {
                        let object = self
                            .objects
                            .get_mut(&args[0])
                            .ok_or_else(|| Error::new(vm.pc, "任务对象不存在"))?;
                        if object.control().deletion_requested != 0 {
                            return Err(Error::new(vm.pc, "已删除任务的排队返回值未初始化"));
                        }
                        object.enqueue_message(message)?;
                    }
                    return Ok(NativeReturn::Value(1));
                }
                // 正在执行 native 的自身 VM 不处于消息等待状态。
                let value = if args[0] == self.host.task_id {
                    0
                } else {
                    self.message_target(args[0], &mut message)?
                };
                Ok(NativeReturn::Value(value))
            }
            109 => Ok(NativeReturn::Value(
                self.command_target(args[0], 96, [0; 3])?,
            )),
            110 => {
                self.command_target(args[0], 97, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            72 | 73 => {
                let count = read::sized(args[2], vm.pc)?;
                if args[0] == self.host.task_id {
                    let (destination, source) = if index == 72 {
                        (args[3], args[1])
                    } else {
                        (args[1], args[3])
                    };
                    crate::kcs::copy_memory(
                        vm,
                        destination,
                        source,
                        count,
                        self.host.memory_copy_mode,
                    )?;
                } else {
                    if !self.require_reachable(args[0]) {
                        return Err(Error::new(vm.pc, "任务存储目标不存在"));
                    }
                    let other = self
                        .objects
                        .get_mut(&args[0])
                        .and_then(|object| object.interpreter_mut())
                        .ok_or_else(|| Error::new(vm.pc, "任务对象不存在、正在执行或没有 VM"))?;
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

impl<F: ObjectFactory> Host for ObjectHost<'_, F> {
    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.resources
            .drawing
            .target
            .as_ref()
            .map(|t| crate::graphics::CanvasLayout::new(t.tile_size(), t.grid(), t.tile_size()))
            .transpose()
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        Some(&mut self.resources.drawables)
    }

    fn resource_services(&mut self) -> Option<&mut dyn crate::kcs::ResourceServices> {
        match &mut self.resources.services {
            Some(services) => Some(services.as_mut()),
            None => None,
        }
    }

    fn next_task_child(&self, previous: Option<u32>) -> Result<Option<u32>> {
        Ok(self
            .tree
            .next_child(self.host.task_id, previous.map(|id| id as i32)))
    }

    fn dispatch_task_command(
        &mut self,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        route_command(self.store(), origin, command, buffers, output)
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> TaskEffects {
        let Some(task) = self.running.get_mut(&self.host.task_id) else {
            return TaskEffects::default();
        };
        let effects = std::mem::take(&mut task.effects);
        task.remaining_messages = Some(
            read::MAX_ITEMS.saturating_sub(queued_messages.saturating_add(effects.messages.len())),
        );
        effects
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.host.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.host.task_id = id;
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.context.clear();
        self.context.extend_from_slice(context);
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if matches!(index, 78 | 79 | 80 | 82 | 83 | 84 | 100 | 101) {
            return self.graphics_native(index, arguments, vm);
        }
        if matches!(index, 66 | 67 | 69 | 70 | 72 | 73 | 75 | 76 | 109 | 110) {
            return self.task_native(index, arguments, vm);
        }
        if matches!(
            index,
            111 | 112 | 114 | 115 | 117 | 365 | 366 | 368 | 590..=593
        ) {
            return self.resources.call(self.host.task_id, index, arguments, vm);
        }
        call_native(
            &mut *self.numbers,
            &mut *self.host,
            None,
            index,
            arguments,
            vm,
        )
    }
}

/// 树调度使用的生命周期服务；每轮从对象存储取出目标对象执行。
struct TaskLifecycle<'a, F: ObjectFactory> {
    numbers: &'a mut expression::State,
    host: &'a mut HostState,
    resources: &'a mut Resources,
    objects: &'a mut BTreeMap<u32, Box<dyn TaskObject>>,
    running: &'a mut BTreeMap<u32, RunningTask>,
    factory: &'a mut F,
    budget: usize,
    limit: usize,
}

impl<F: ObjectFactory> TaskLifecycle<'_, F> {
    fn store<'a>(&'a mut self, tree: &'a mut TaskTree) -> Store<'a, F> {
        Store {
            numbers: &mut *self.numbers,
            host: &mut *self.host,
            resources: &mut *self.resources,
            tree,
            objects: &mut *self.objects,
            running: &mut *self.running,
            factory: &mut *self.factory,
            budget: self.budget,
            limit: self.limit,
        }
    }
}

impl<F: ObjectFactory> Lifecycle for TaskLifecycle<'_, F> {
    fn state(&mut self, id: u32) -> Result<(&mut Control, &mut TaskClock)> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or_else(|| Error::new(0, "任务对象不存在"))?;
        Ok(object.state())
    }

    fn update_body(&mut self, tree: &mut TaskTree, id: u32, increment: u32) -> Result<()> {
        run_nested(self.store(tree), id, Execution::Body { increment })
    }

    fn request_delete(&mut self, tree: &mut TaskTree, command: Command) -> Result<()> {
        run_nested(self.store(tree), command.target, Execution::Delete)
    }

    fn mark_destroying(&mut self, tree: &mut TaskTree, id: u32) -> Result<()> {
        run_nested(self.store(tree), id, Execution::MarkDestroying)
    }

    fn release(&mut self, id: u32) -> Result<()> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or_else(|| Error::new(0, "任务对象不存在"))?;
        object.release()?;
        self.resources.images.release_owner(id);
        self.resources.drawables.remove(id);
        self.objects.remove(&id);
        Ok(())
    }
}
