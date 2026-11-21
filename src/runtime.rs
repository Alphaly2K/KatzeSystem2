//! 任务树、对象存储与普通脚本宿主的整合。
//!
//! 本模块把任务索引、基类控制、生命周期回调和真实对象存储连接起来，替代此前只能逐项手工搭建的适配层。
//! 对象类型由 [`ObjectFactory`] 创建；提供普通 KCS、基类及 [`crate::graphics::GraphicsFactory`] 的二维与图像列表对象。
//! 其他类型需要对应子系统按 [`TaskObject`] 协议实现，缺失时创建请求返回诊断。
//!
//! 同步消息、删除通知和对象命令在返回前完成预算续跑。普通对象执行期间仍接收删除标记与排队消息，
//! 向执行中普通 VM 同步发送消息返回零。输入同步回调开放同一份区域、键和基类命令状态，图形通知开放响应槽及几何查询；
//! 其他需要直接借用执行中对象的操作返回诊断。

use crate::kcs::{Host, HostState, Program, TaskEffects};
use crate::task::{Command, CommandBuffers, Control, Lifecycle, TaskClock, TaskTree};
use crate::{Error, Result, expression, read};
use std::collections::{BTreeMap, BTreeSet};

mod audio;
mod backlog;
mod bmb;
mod cst;
mod debug_window;
mod diagnostics;
pub use crate::host::diagnostics::{
    DiagnosticTrace, NativeInvocation, NativeTraceEvent, QuizDebugInput, QuizDebugSession,
    ScriptLocation, ScriptMessages, ScriptPosition, TraceCategory,
};
use crate::host::trace_detail;
mod emocon;
mod export;
pub mod fes;
pub use cst::CstContext;
mod effect;
mod graphics;
mod host;
mod image_cursor;
mod input;
mod language;
mod message;
mod motion;
mod movie;
mod native;
mod particle;
mod text_window;
pub use motion::MotionSnapshot;
mod frame;
pub use crate::host::graphics::SequenceCapture;
mod game_tasks;
mod image_animation;
mod image_data;
mod image_export;
mod image_pack;
mod image_resources;
mod object;
mod ordinary;
mod platform_job;
mod presentation;
mod readback;
mod resources;
mod root;
mod save;
pub use game_tasks::{GameFactory, GameTasks, ResourcePrograms, TaskRuntime};
mod state_records;
mod system_tasks;
mod task_host;
mod transition;
mod window;
pub use crate::host::{
    ClipboardRequest, ClipboardText, EffectPlayer, EffectServices, ExportDestination,
    ExportServices, PlatformJob, PlatformJobRequest, PlatformJobServices, WindowServices,
};

use resources::Resources;

pub use object::{BaseObject, TaskObject};
pub use ordinary::OrdinaryObject;
pub use root::RootObject;

/// 基线工厂中的对象类型编号。数值来自静态构造与类型追踪，具体工厂仍需逐类接入。
pub mod kind {
    pub const BASE: u32 = 0;
    pub const ROOT: u32 = 1;
    pub const PLANE: u32 = 2;
    pub const BUTTON: u32 = 6;
    pub const TEXT_WINDOW: u32 = 8;
    pub const INPUT: u32 = 11;
    pub const ORDINARY: u32 = 12;
    pub const SOUND: u32 = 13;
    pub const IMAGE_LIST: u32 = 15;
    pub const PARTICLE_3D: u32 = 16;
    pub const WAVE: u32 = 18;
    pub const WIPE: u32 = 26;
    pub const FADE: u32 = 27;
    pub const WIPE_ALPHA: u32 = 28;
    pub const FES: u32 = 34;
    pub const RENDER_BLUR: u32 = 35;
    pub const RENDER_MOSAIC: u32 = 36;
    pub const RENDER_SHADE: u32 = 37;
    pub const RENDER_DRAW: u32 = 38;
    pub const RWIPE: u32 = 39;
    pub const SCROLL2: u32 = 41;
    pub const SLIDER: u32 = 42;
    pub const EXPORT: u32 = 45;
    pub const STRING_PLANE: u32 = 48;
    pub const RENDER_WMOSAIC: u32 = 50;
    pub const RENDER_FMOSAIC: u32 = 51;
    pub const RENDER_WBREAK: u32 = 52;
    pub const QUIZ_DEBUG: u32 = 53;
    pub const PFLIP: u32 = 57;
    pub const CFLIP: u32 = 58;
    pub const PLATFORM_PLUGIN: u32 = 59;
    pub const IMAGE_IMPORT: u32 = 60;
    pub const EFFEKSEER: u32 = 61;
    pub const STRING_PLANE_UTF8: u32 = 63;
    pub const CAMERA_2D: u32 = 65;
    /// 粒子任务（`kcTaskParticle`）。
    pub const PARTICLE: u32 = PARTICLE_3D;
}

/// 默认的单次推进预算；预算耗尽时同步入口继续推进到完成。
pub const DEFAULT_BUDGET: usize = 4096;
/// 连续耗尽推进预算时的同步执行保护阈值。
pub const DEFAULT_LIMIT: usize = 1 << 20;

/// 按对象类型创建对象；未实现的类型返回诊断。
pub trait ObjectFactory {
    /// 新建普通任务与 FES 任务共用的初始外部区；包装工厂应转交同一视图。
    fn external_storage(&self) -> crate::memory::SharedMemory {
        crate::memory::SharedMemory::default()
    }

    /// `id` 为已选定的任务编号，`name` 为命令 66 展开后的名称字节。
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>>;

    /// 构造尚无 VM 的普通任务；后续通过命令 161 装载。
    fn create_unloaded_ordinary(&mut self, _id: u32) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "对象工厂未提供未装载普通任务"))
    }

    fn load_kcs_program(&mut self, _name: &[u8]) -> Result<Option<Program>> {
        Err(Error::new(0, "对象工厂未提供 KCS 程序装载"))
    }
}

/// 按名称提供已解析的 KCS 程序。
pub trait ProgramLoader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>>;
}

/// 创建普通 KCS 和脚本导出任务；其他类型由子系统工厂接入。
pub struct KcsFactory<L> {
    pub programs: L,
    /// 新建普通任务与 FES 任务持有的共享外部区。
    pub external: crate::memory::SharedMemory,
}

impl<L: ProgramLoader> KcsFactory<L> {
    pub fn new(programs: L) -> Self {
        Self {
            programs,
            external: crate::memory::SharedMemory::default(),
        }
    }
}

impl<L: ProgramLoader> ObjectFactory for KcsFactory<L> {
    fn external_storage(&self) -> crate::memory::SharedMemory {
        self.external.clone()
    }

    fn create_unloaded_ordinary(&mut self, id: u32) -> Result<Box<dyn TaskObject>> {
        Ok(Box::new(OrdinaryObject::unloaded_shared(
            id,
            self.external.clone(),
        )))
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<Program>> {
        self.programs.load(name)
    }

    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::EXPORT {
            return Ok(Box::new(export::ExportObject::new(id)));
        }
        if kind == kind::QUIZ_DEBUG {
            return Ok(Box::new(debug_window::QuizDebugObject::default()));
        }
        if matches!(kind, kind::PLATFORM_PLUGIN | kind::IMAGE_IMPORT) {
            return Ok(Box::new(platform_job::PlatformJobObject::new(id, kind)));
        }
        if kind == kind::EFFEKSEER {
            return Ok(Box::new(effect::EffectObject::new(id)));
        }
        if kind != kind::ORDINARY {
            return Err(Error::new(0, format!("对象类型 {kind} 需要对应子系统工厂")));
        }
        let program = self
            .programs
            .load(name)?
            .ok_or_else(|| Error::new(0, "KCS 程序不存在"))?;
        Ok(Box::new(OrdinaryObject::from_program(
            id,
            program,
            self.external.clone(),
        )?))
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
    configuration_notification: Option<BTreeSet<u32>>,
    interface_language_refreshed: bool,
}

impl<F: ObjectFactory> Runtime<F> {
    /// 以已经创建好的根对象建立任务树；根对象编号用于后续自动编号与查询。
    pub fn new(
        root_id: u32,
        root_kind: u32,
        mut root: Box<dyn TaskObject>,
        mut numbers: expression::State,
        factory: F,
    ) -> Self {
        let mut host = HostState::default();
        host.configured_shared_slots = numbers.shared.len() as u32;
        let resources = Resources::with_handles(host.handle_allocator());
        root.initialize_language(host.languages.selection());
        if root.shared_numbers_available() == Some(true) {
            if let Some(values) = root.local_numbers() {
                numbers.register_local(root_id, values);
            }
            if let Some(values) = root.named_values() {
                numbers.bind_named(root_id, values);
            }
        }
        Self {
            tree: TaskTree::new(root_id, root_kind),
            objects: BTreeMap::from([(root_id, root)]),
            running: BTreeMap::new(),
            numbers,
            host,
            resources,
            factory,
            budget: DEFAULT_BUDGET,
            limit: DEFAULT_LIMIT,
            configuration_notification: None,
            interface_language_refreshed: false,
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

    /// 配置脚本内部画面捕获使用的投影、父变换和纹理取样。
    /// 调用方应与显式 render_frame 使用同一组参数，并在视图变化时更新。
    pub fn set_frame_view(
        &mut self,
        projection: crate::graphics::Projection,
        parent: [f32; 16],
        filter: crate::graphics::TextureFilter,
    ) {
        self.resources
            .compositor
            .set_frame_view(projection, parent, filter);
    }

    /// Set the logical surface size in pixels used by script and capture queries.
    /// The host manages its presentation size and the runtime canvas separately.
    pub fn set_surface_size(&mut self, size: [u32; 2]) {
        self.host.surface_size = size;
    }

    /// 以已配置的帧视图和当前画布合成绘制列表。
    pub fn compose_frame(&mut self) -> Result<crate::graphics::DrawList> {
        let mut frame = self.resources.compose_frame(None)?;
        if self.uses_image_cursor() {
            self.resources.append_image_cursor(&mut frame)?;
        }
        Ok(frame)
    }

    /// 设置内部捕获所读取的画面区域；None 使用 surface_size 定义的整幅画面。
    pub fn set_frame_capture_rectangle(&mut self, rectangle: Option<crate::graphics::Rect>) {
        self.resources.capture_rectangle = rectangle;
    }

    /// Pixels from the last successful script readback. Skipped or failed
    /// readbacks leave the previous frame available.
    pub fn offscreen_surface(&self) -> Option<&crate::host::graphics::Surface> {
        self.resources.offscreen_surface.as_ref()
    }

    pub fn images(&self) -> &crate::graphics::Images {
        &self.resources.images
    }

    pub fn images_mut(&mut self) -> &mut crate::graphics::Images {
        &mut self.resources.images
    }

    /// 整幅重绘图像列表并复制到当前画布；提交受 DrawingContext 的模式与开关门控。
    pub fn redraw_image_list(&mut self, list: &mut crate::graphics::ImageList) -> Result<bool> {
        list.invalidate_all();
        self.resources.update_image_list(list, true, true, None)
    }

    /// 更新并提交列表脏区；关闭上下文时仍更新自有图像并消费脏区。
    pub fn update_image_list(&mut self, list: &mut crate::graphics::ImageList) -> Result<bool> {
        self.resources.update_image_list(list, true, false, None)
    }

    /// 365 装载或 FES 保留的原始图像字节，存活至最后一份引用释放。
    pub fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.resources.raw.image_data(handle)
    }

    pub fn drawing(&self) -> &crate::graphics::DrawingContext {
        self.resources.compositor.drawing()
    }

    pub fn drawing_mut(&mut self) -> &mut crate::graphics::DrawingContext {
        self.resources.compositor.drawing_mut()
    }

    pub fn drawables(&self) -> &crate::graphics::Drawables {
        self.resources.compositor.drawables()
    }

    pub fn drawables_mut(&mut self) -> &mut crate::graphics::Drawables {
        self.resources.compositor.drawables_mut()
    }

    /// 借用脚本、媒体任务和最终帧提交共用的合成状态。
    pub fn compositor(&self) -> &crate::graphics::Compositor {
        &self.resources.compositor
    }

    pub fn compositor_mut(&mut self) -> &mut crate::graphics::Compositor {
        &mut self.resources.compositor
    }

    /// 更新完成后显式绘制基础纯色及渐变图元；帧清空和平台呈现由调用者驱动。
    pub fn render_triangles(
        &mut self,
        projection: &crate::graphics::Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.resources
            .compositor
            .render_triangles(projection, parent)
    }

    pub fn host(&self) -> &HostState {
        &self.host
    }

    /// 借用独立回看记录；释放任务不会回收日志句柄。
    pub fn backlog(&self, handle: u32) -> Option<&crate::backlog::Backlog> {
        self.resources.backlogs.get(&handle)
    }

    /// 探针枚举已存在的 CST 上下文。
    pub fn cst_handles(&self) -> Vec<u32> {
        self.resources.cst_contexts.keys().copied().collect()
    }

    pub fn cst_context(&self, handle: u32) -> Option<&CstContext> {
        self.resources.cst_contexts.get(&handle)
    }

    pub fn cst_context_mut(&mut self, handle: u32) -> Option<&mut CstContext> {
        self.resources.cst_contexts.get_mut(&handle)
    }

    pub fn motion_snapshot(&self, handle: u32) -> Option<&MotionSnapshot> {
        self.resources.motion_snapshots.get(&handle)
    }

    pub fn motion_controller(&self, handle: u32) -> Option<&crate::kcs::MotionQueue> {
        self.resources.motion_controllers.get(&handle)
    }

    pub fn motion_controller_mut(&mut self, handle: u32) -> Option<&mut crate::kcs::MotionQueue> {
        self.resources.motion_controllers.get_mut(&handle)
    }

    /// 修改共享宿主状态；整体替换时须沿用原状态的 `handle_allocator()`。
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
    /// KcsFactory 的此入口按名称预先装载；构造后装载使用 create_unloaded_ordinary。
    pub fn create(&mut self, parent: u32, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        create_object(self.store(), parent, kind, name, context)
    }

    /// 构造没有 VM 的普通任务，供命令 161 在登记后装载。
    pub fn create_unloaded_ordinary(&mut self, parent: u32, context: &[u8]) -> Result<u32> {
        create_unloaded_ordinary(self.store(), parent, context)
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

    pub fn input_state(&self, id: u32) -> Option<&crate::input::InputState> {
        self.objects
            .get(&id)
            .and_then(|object| object.input_state())
    }

    pub fn input_state_mut(&mut self, id: u32) -> Option<&mut crate::input::InputState> {
        self.objects
            .get_mut(&id)
            .and_then(|object| object.input_state_mut())
    }

    /// 绑定同步设备，并读取已有输入对象的初始设备位置。
    pub fn set_input_device(
        &mut self,
        device: impl crate::input::InputDevice + 'static,
    ) -> Result<()> {
        self.resources.input_device = Some(Box::new(device));
        for object in self.objects.values_mut() {
            if let Some(state) = object.input_state_mut() {
                let position = self
                    .resources
                    .input_device
                    .as_mut()
                    .unwrap()
                    .pointer()?
                    .position;
                state.initialize_pointer(position);
            }
        }
        Ok(())
    }

    pub fn input_coordinates(&self) -> &crate::input::CoordinateMap {
        &self.resources.input_coordinates
    }
    pub fn input_coordinates_mut(&mut self) -> &mut crate::input::CoordinateMap {
        &mut self.resources.input_coordinates
    }
    pub fn set_input_pointer_control(&mut self, enabled: bool) {
        self.resources.input_pointer_control = enabled;
    }

    /// 以已经转换的逻辑坐标同步分派一条输入事件。
    pub fn dispatch_input_key(&mut self, id: u32, event: crate::input::KeyEvent) -> Result<bool> {
        let mut consumed = false;
        self.with_input(id, &mut |state, host| {
            consumed = state.dispatch_key(id, event, host)?;
            Ok(())
        })?;
        Ok(consumed)
    }

    /// 重算指针选择，并在返回前完成进入／离开通知。
    pub fn update_input_selection(&mut self, id: u32, point: Option<[i32; 2]>) -> Result<()> {
        self.with_input(id, &mut |state, host| {
            state.update_pointer_selection(id, point, host)
        })
    }

    /// 同步分派共享前后点缓冲区的移动通知。
    pub fn dispatch_input_motion(
        &mut self,
        id: u32,
        motion: crate::input::PointerMotion,
    ) -> Result<()> {
        self.with_input(id, &mut |state, host| {
            state.dispatch_pointer_motion(id, motion, host)
        })
    }

    /// 按实际查询顺序读取设备；同步回调可以修改本次后续键开关和区域。
    pub fn scan_input_keys(
        &mut self,
        id: u32,
        point: [i32; 2],
        mut sample: impl FnMut(u32, u32) -> Result<crate::input::KeySample>,
    ) -> Result<bool> {
        let mut consumed = false;
        self.with_input(id, &mut |state, host| {
            consumed = state.scan_keys(id, point, &mut sample, host)?;
            Ok(())
        })?;
        Ok(consumed)
    }

    fn with_input(&mut self, id: u32, action: &mut InputAction<'_>) -> Result<()> {
        run_nested(self.store(), id, Execution::Input { action })
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
            fes_scene: None,
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
            fes_scene: None,
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
    store: Store<'_, F>,
    parent: u32,
    kind: u32,
    name: &[u8],
    context: &[u8],
) -> Result<u32> {
    create_object_impl(store, parent, kind, name, context, false)
}

fn create_unloaded_ordinary<F: ObjectFactory>(
    store: Store<'_, F>,
    parent: u32,
    context: &[u8],
) -> Result<u32> {
    create_object_impl(store, parent, kind::ORDINARY, &[], context, true)
}

fn create_object_impl<F: ObjectFactory>(
    store: Store<'_, F>,
    parent: u32,
    kind: u32,
    name: &[u8],
    context: &[u8],
    unloaded: bool,
) -> Result<u32> {
    let Store {
        tree,
        objects,
        factory,
        resources,
        host,
        numbers,
        running,
        fes_scene,
        budget,
        limit,
    } = store;
    if tree.node(parent).is_none() {
        return Err(Error::new(0, "任务父级不存在"));
    }
    let id = tree.next_id();
    let mut object = if unloaded {
        factory.create_unloaded_ordinary(id)?
    } else if kind == kind::ORDINARY && resources.resource_programs {
        let name = resources.resolve_ordinary_program(name)?;
        let program = resources
            .read_ordinary_program(&name)?
            .ok_or_else(|| Error::new(0, "KCS 程序不存在"))?;
        Box::new(OrdinaryObject::from_program(
            id,
            program,
            factory.external_storage(),
        )?)
    } else {
        factory.create(id, kind, name)?
    };
    object.initialize_language(host.languages.selection());
    object.set_creation_context(context)?;
    if let Some(audio) = object.audio_state_mut() {
        audio.set_decode_mode(resources.decode_mode);
        if let Some(groups) = &host.sound_groups {
            audio.set_groups(groups)?;
        }
    }
    if let (Some(state), Some(device)) = (object.input_state_mut(), resources.input_device.as_mut())
    {
        state.initialize_pointer(device.pointer()?.position);
    }
    object.initialize_resources(&mut ObjectHost {
        numbers,
        host,
        resources,
        tree,
        objects,
        running,
        factory,
        fes_scene,
        context: Vec::new(),
        budget,
        limit,
    })?;
    object.register_graphics(&mut resources.compositor.drawables)?;
    tree.insert(parent, id, kind)?;
    if object.shared_numbers_available() == Some(true) {
        if let Some(values) = object.local_numbers() {
            numbers.register_local(id, values);
        }
        if let Some(values) = object.named_values() {
            numbers.bind_named(id, values);
        }
    }
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
    PrepareRelease,
    Input {
        action: &'a mut InputAction<'b>,
    },
}

type InputAction<'a> = dyn FnMut(&mut crate::input::InputState, &mut dyn Host) -> Result<()> + 'a;

struct RunningTask {
    deleted: bool,
    property: u32,
    property_changed: bool,
    deferred_delete: bool,
    clock: TaskClock,
    paused_changed: Option<i32>,
    context: Option<Vec<u8>>,
    shared_numbers: Option<bool>,
    local_numbers: Option<crate::expression::LocalNumbers>,
    external: Option<crate::memory::SharedMemory>,
    remaining_messages: Option<usize>,
    memory: Option<crate::kcs::MemoryLoan>,
    effects: TaskEffects,
}

// 媒体命令返回前可能同步通知正在更新的 FES，回调借用同一份场景。
struct FesSceneLoan<'a> {
    owner: u32,
    scene: &'a mut fes::Scene,
}

impl FesSceneLoan<'_> {
    fn reborrow(&mut self) -> FesSceneLoan<'_> {
        FesSceneLoan {
            owner: self.owner,
            scene: self.scene,
        }
    }
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
    fes_scene: Option<FesSceneLoan<'a>>,
    budget: usize,
    limit: usize,
}

impl<F: ObjectFactory> Store<'_, F> {
    fn reborrow(&mut self) -> Store<'_, F> {
        Store {
            numbers: self.numbers,
            host: self.host,
            resources: self.resources,
            tree: self.tree,
            objects: self.objects,
            running: self.running,
            factory: self.factory,
            fes_scene: self.fes_scene.as_mut().map(FesSceneLoan::reborrow),
            budget: self.budget,
            limit: self.limit,
        }
    }
}

/// 取出目标对象执行一次，完成后放回并恢复宿主记录的当前任务状态。
fn run_nested<F: ObjectFactory>(
    store: Store<'_, F>,
    target: u32,
    mut execution: Execution<'_, '_>,
) -> Result<()> {
    let Store {
        numbers,
        host,
        resources,
        tree,
        objects,
        running,
        factory,
        mut fes_scene,
        budget,
        limit,
    } = store;
    if let Execution::Command {
        kind: kind::ORDINARY,
        command: Command { code: 176, .. },
        buffers,
        propagate,
        ..
    } = &mut execution
    {
        let deleted = running.get(&target).is_some_and(|task| task.deleted)
            || objects
                .get(&target)
                .is_some_and(|object| object.control().deletion_requested != 0);
        if !deleted {
            let available = u32::from(resources.legacy_cst.contains_key(&target)).to_le_bytes();
            for destination in [&mut buffers.data0, &mut buffers.data1]
                .into_iter()
                .flatten()
            {
                destination
                    .get_mut(..4)
                    .ok_or_else(|| Error::new(0, "CST 上下文查询缓冲区不足"))?
                    .copy_from_slice(&available);
            }
        }
        **propagate = false;
        return Ok(());
    }
    if let (
        Some(task),
        Execution::Command {
            kind,
            command,
            buffers,
            propagate,
            ..
        },
    ) = (running.get(&target), &mut execution)
    {
        if let (177, Some(available)) = (command.code, task.shared_numbers) {
            save::query_shared_numbers_available(available, task.deleted, buffers)?;
            **propagate = false;
            return Ok(());
        }
        if let (25, Some(context)) = (command.code, &task.context) {
            if !task.deleted && command.data[1] != 0 {
                let destination = buffers
                    .data1
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "执行中任务上下文输出缓冲区缺失"))?;
                if destination.len() <= context.len() {
                    return Err(Error::new(0, "任务创建上下文输出容量不足"));
                }
                destination[..context.len()].copy_from_slice(context);
                destination[context.len()] = 0;
            }
            **propagate = false;
            return Ok(());
        }
        if matches!(*kind, kind::PLANE | kind::EFFEKSEER) && matches!(command.code, 61 | 62) {
            if !task.deleted {
                crate::graphics::query_geometry(
                    &resources.compositor.drawables,
                    target,
                    command,
                    buffers,
                )?;
            }
            **propagate = false;
            return Ok(());
        }
        if *kind == kind::ORDINARY && command.code == 120 {
            **propagate = !task.deleted;
            return Ok(());
        }
    }
    if let Some(task) = running.get_mut(&target) {
        if let Execution::Command {
            command,
            output,
            propagate,
            ..
        } = &mut execution
        {
            if matches!(command.code, 23 | 24 | 29) {
                if !task.deleted {
                    match command.code {
                        23 => **output = task.property,
                        24 => {
                            task.property = command.data[1];
                            task.property_changed = true;
                        }
                        _ => {
                            task.clock.paused = command.data[1] as i32;
                            task.paused_changed = Some(command.data[1] as i32);
                        }
                    }
                }
                **propagate = false;
                return Ok(());
            }
        }
        let propagate = match &mut execution {
            Execution::Delete if task.deferred_delete => Some(None),
            Execution::Command {
                command, propagate, ..
            } if command.code == 3 && task.deferred_delete => Some(Some(propagate)),
            _ => None,
        };
        if let Some(propagate) = propagate {
            if !task.deleted {
                task.deleted = true;
                task.effects.delete = true;
            }
            if let Some(propagate) = propagate {
                **propagate = false;
            }
            return Ok(());
        }
    }
    if fes_scene.as_ref().is_some_and(|loan| loan.owner == target) {
        if let Execution::Command {
            command,
            buffers,
            output,
            propagate,
            ..
        } = &mut execution
        {
            if command.code == 344 {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let bytes = buffers
                        .data0
                        .as_deref()
                        .filter(|bytes| bytes.len() >= 36)
                        .ok_or_else(|| Error::new(0, "FES 动态创建名称及数量缓冲区不足"))?;
                    let name = crate::read::zstr(&bytes[..32], 0)?;
                    fes_scene.as_mut().unwrap().scene.define_object(
                        command.data[1],
                        name,
                        crate::read::u32_at(bytes, 32)?,
                        numbers,
                    )?;
                }
                return Ok(());
            }
            if command.code == 356 {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let scene = &mut fes_scene.as_mut().unwrap().scene;
                    let mut nested = ObjectHost {
                        numbers,
                        host,
                        resources,
                        tree,
                        objects,
                        running,
                        factory,
                        fes_scene: None,
                        context: Vec::new(),
                        budget,
                        limit,
                    };
                    scene.bind_image_resource(
                        target,
                        command.data[1],
                        buffers.data0.as_deref(),
                        &mut nested,
                    )?;
                }
                return Ok(());
            }
            if matches!(command.code, 351 | 357 | 358) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let scene = &mut fes_scene.as_mut().unwrap().scene;
                    if command.code == 351 {
                        let name = crate::read::zstr(
                            buffers
                                .data0
                                .as_deref()
                                .ok_or_else(|| Error::new(0, "FES 表达式名称缓冲区缺失"))?,
                            0,
                        )?;
                        scene.register_named_value(name, command.data[1], numbers)?;
                    } else {
                        scene.raw_name_command(command.code, command.data[1], buffers)?;
                    }
                }
                return Ok(());
            }
            if command.code == 345 {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let header = crate::read::zstr(
                        buffers
                            .data0
                            .as_deref()
                            .ok_or_else(|| Error::new(0, "FES 动态属性列头缓冲区缺失"))?,
                        0,
                    )?;
                    fes_scene.as_mut().unwrap().scene.initialize_object_line(
                        header,
                        buffers.data1.as_deref(),
                        numbers,
                        |name| host.languages.expand_image(name),
                        |name| fes::read_table(resources, name),
                    )?;
                }
                return Ok(());
            }
            if command.code == 343 {
                **propagate = false;
                if running.get(&target).is_some_and(|task| task.deleted) {
                    return Ok(());
                }
                let block = crate::text::cp932(crate::read::zstr(
                    buffers
                        .data1
                        .as_deref()
                        .ok_or_else(|| Error::new(0, "FES 块名称缓冲区缺失"))?,
                    0,
                )?)?;
                let context = buffers
                    .data0
                    .as_deref()
                    .map(|bytes| {
                        if bytes.len() < 36 {
                            return Err(Error::new(0, "FES 块对象上下文容量不足"));
                        }
                        Ok(crate::fes::ObjectReference {
                            name: crate::text::cp932(crate::read::zstr(&bytes[..32], 0)?)?,
                            bank: read::u32_at(bytes, 32)? as i32,
                        })
                    })
                    .transpose()?;
                let loan = fes_scene.take().unwrap();
                let now = running
                    .get(&target)
                    .map_or(host.task_time, |task| task.clock.elapsed());
                let saved = (host.task_id, host.task_time, host.task_origin);
                host.task_id = target;
                if let Some(task) = running.get(&target) {
                    host.task_time = task.clock.time;
                    host.task_origin = task.clock.origin;
                }
                let mut store = Store {
                    numbers,
                    host,
                    resources,
                    tree,
                    objects,
                    running,
                    factory,
                    fes_scene: None,
                    budget,
                    limit,
                };
                let result = drain(budget, limit, || {
                    store.dispatch_borrowed_fes_block(
                        target,
                        loan.scene,
                        context.as_ref(),
                        &block,
                        now,
                    )
                });
                store.host.task_id = saved.0;
                store.host.task_time = saved.1;
                store.host.task_origin = saved.2;
                return result;
            }
            if matches!(command.code, 349 | 350) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    fes_scene.as_mut().unwrap().scene.parameter_record_command(
                        command.code,
                        command.data[0],
                        buffers.data1.as_deref_mut(),
                        numbers.active_local(),
                        output,
                    )?;
                }
                return Ok(());
            }
            if matches!(command.code, 352 | 353) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let caller = command.data[1];
                    let (from, to) = if command.code == 352 {
                        (caller, target)
                    } else {
                        (target, caller)
                    };
                    numbers.copy_local(from, to)?;
                }
                return Ok(());
            }
            if matches!(command.code, 354 | 355) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    fes_scene
                        .as_mut()
                        .unwrap()
                        .scene
                        .control_status_command(command.code, output);
                }
                return Ok(());
            }
            if matches!(command.code, 342 | 346 | 347) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let scene = &mut fes_scene.as_mut().unwrap().scene;
                    if matches!(command.code, 346 | 347) {
                        numbers.select_context(
                            scene.interpreter().local_numbers(),
                            scene.interpreter().named_context(),
                        );
                    }
                    let index = command.data[0] as usize;
                    if command.code == 342 {
                        scene.set_system_value("_SAVEOBJ_", command.data[1] as i32);
                    } else if command.code == 346 {
                        if let Some(value) = scene.parameter_values_mut().get_mut(index) {
                            *value = command.data[1] as i32;
                        }
                    } else {
                        **output = scene.parameter_values().get(index).copied().unwrap_or(0) as u32;
                    }
                }
                return Ok(());
            }
            if matches!(command.code, 27 | 28) {
                **propagate = false;
                if running.get(&target).is_some_and(|task| task.deleted) {
                    return Ok(());
                }
                let loan = fes_scene.take().unwrap();
                let saved = (host.task_id, host.task_time, host.task_origin);
                let result = {
                    let mut nested = ObjectHost {
                        numbers,
                        host,
                        resources,
                        tree,
                        objects,
                        running,
                        factory,
                        fes_scene: None,
                        context: Vec::new(),
                        budget,
                        limit,
                    };
                    nested.host.task_id = target;
                    let clock = nested.running[&target].clock.clone();
                    nested.set_task_clock(&clock);
                    fes::dispatch_borrowed_control_event(
                        target,
                        loan.scene,
                        command,
                        &mut nested,
                        budget,
                        limit,
                    )
                };
                host.task_id = saved.0;
                host.task_time = saved.1;
                host.task_origin = saved.2;
                return result;
            }
        }
    }
    if let Some(mut button) = resources.button_callbacks.remove(&target) {
        let saved = (host.task_id, host.task_time, host.task_origin);
        let result = {
            let mut nested = ObjectHost {
                numbers,
                host,
                resources,
                tree,
                objects,
                running,
                factory,
                fes_scene,
                context: Vec::new(),
                budget,
                limit,
            };
            nested.host.task_id = target;
            nested.set_task_clock(button.clock());
            match execution {
                Execution::Command {
                    kind,
                    command,
                    buffers,
                    output,
                    propagate,
                } => drain(budget, limit, || {
                    let outcome = button.dispatch_command_with_buffers(
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
                    drain(budget, limit, || button.request_delete(&mut nested, budget))
                }
                Execution::MarkDestroying => drain(budget, limit, || {
                    button.mark_destroying(&mut nested, budget)
                }),
                _ => Err(Error::new(0, "按钮回调期间不支持重新执行该任务")),
            }
        };
        resources.button_callbacks.insert(target, button);
        host.task_id = saved.0;
        host.task_time = saved.1;
        host.task_origin = saved.2;
        return result;
    }
    if resources.graphics_responses.contains_key(&target) {
        if let Execution::Command {
            kind,
            command,
            output,
            propagate,
            ..
        } = &mut execution
        {
            if command.code == 95 {
                *resources.graphics_responses.get_mut(&target).unwrap() = command.data[1];
                **propagate = false;
                return Ok(());
            }
            if matches!(*kind, kind::BUTTON | kind::PLANE) && command.code == 96 {
                **propagate = false;
                if running.get(&target).is_some_and(|task| task.deleted) {
                    return Ok(());
                }
                let mut store = Store {
                    numbers,
                    host,
                    resources,
                    tree,
                    objects,
                    running,
                    factory,
                    fes_scene,
                    budget,
                    limit,
                };
                let mut input = 0;
                route_command(
                    store.reborrow(),
                    target,
                    Command {
                        target: 0,
                        sender: target,
                        code: 34,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut input,
                )?;
                let mut value = 0;
                route_command(
                    store,
                    target,
                    Command {
                        target: input,
                        sender: target,
                        code: 137,
                        data: [target, 0, command.data[2]],
                    },
                    &mut CommandBuffers::default(),
                    &mut value,
                )?;
                **output = value;
                return Ok(());
            }
        }
        return Err(Error::new(0, "图形回调期间仅支持响应槽回写和输入状态查询"));
    }
    if let Some(mut state) = resources.input_callbacks.remove(&target) {
        let saved = (host.task_id, host.task_time, host.task_origin);
        let result = {
            let mut nested = ObjectHost {
                numbers,
                host,
                resources,
                tree,
                objects,
                running,
                factory,
                fes_scene,
                context: Vec::new(),
                budget,
                limit,
            };
            nested.host.task_id = target;
            nested.set_task_clock(&state.clock);
            match execution {
                Execution::Command {
                    kind,
                    command,
                    buffers,
                    output,
                    propagate,
                } => state
                    .dispatch_command(kind, command, buffers, output, &mut nested)
                    .map(|value| *propagate = value),
                Execution::Delete | Execution::MarkDestroying => {
                    state.request_delete(nested.input_device())
                }
                _ => Err(Error::new(0, "输入回调期间不支持重新执行该任务")),
            }
        };
        resources.input_callbacks.insert(target, state);
        host.task_id = saved.0;
        host.task_time = saved.1;
        host.task_origin = saved.2;
        return result;
    }
    let mut object = objects.remove(&target).ok_or_else(|| {
        let operation = match &execution {
            Execution::Command { command, .. } => format!("命令 {}", command.code),
            _ => "执行".to_owned(),
        };
        Error::new(
            0,
            format!("任务对象不存在或正在执行（{target}，{operation}）"),
        )
    })?;
    running.insert(
        target,
        RunningTask {
            deleted: object.control().deletion_requested != 0,
            property: object.control().property,
            property_changed: false,
            deferred_delete: object.accepts_deferred_delete(),
            clock: object.clock().clone(),
            paused_changed: None,
            context: object.creation_context().map(<[u8]>::to_vec),
            shared_numbers: object.shared_numbers_available(),
            local_numbers: object.local_numbers(),
            external: object.external_storage(),
            remaining_messages: object
                .queued_messages()
                .map(|messages| read::MAX_ITEMS.saturating_sub(messages.len())),
            memory: None,
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
            fes_scene,
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
            Execution::PrepareRelease => drain(budget, limit, || {
                object.prepare_release(&mut nested, budget)
            }),
            Execution::Input { action } => {
                nested.set_task_clock(object.clock());
                match object.input_state_mut() {
                    Some(state) => action(state, &mut nested),
                    None => Err(Error::new(0, "目标不是输入对象")),
                }
            }
        }
    };
    if let Some(task) = running.remove(&target) {
        if task.property_changed {
            object.state().0.property = task.property;
        }
        if let Some(paused) = task.paused_changed {
            object.state().1.paused = paused;
        }
    }
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
    route_command_mode(
        store,
        origin,
        command,
        buffers,
        output,
        crate::input::InputDelivery::Direct,
        None,
    )
}

fn route_command_mode<F: ObjectFactory>(
    store: Store<'_, F>,
    origin: u32,
    command: Command,
    buffers: &mut CommandBuffers<'_>,
    output: &mut u32,
    delivery: crate::input::InputDelivery,
    mut completed: Option<&mut BTreeSet<u32>>,
) -> Result<bool> {
    let Store {
        numbers,
        host,
        resources,
        tree,
        objects,
        running,
        factory,
        mut fes_scene,
        budget,
        limit,
    } = store;
    let handler = |tree: &mut TaskTree, command: &Command, output: &mut u32| {
        if completed
            .as_ref()
            .is_some_and(|receivers| receivers.contains(&command.target))
        {
            return Ok(true);
        }
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
                fes_scene: fes_scene.as_mut().map(FesSceneLoan::reborrow),
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
        if let Some(receivers) = &mut completed {
            receivers.insert(command.target);
        }
        Ok(propagate)
    };
    match delivery {
        crate::input::InputDelivery::Direct => tree.dispatch(origin, command, output, handler),
        crate::input::InputDelivery::Broadcast => {
            if tree.find_reachable(origin, command.target).is_none() {
                return Ok(false);
            }
            tree.broadcast(origin, None, command, output, handler)?;
            Ok(true)
        }
    }
}

/// 执行中对象的宿主；核心调用由注册表分派，任务调用使用当前对象存储。
struct ObjectHost<'a, F: ObjectFactory> {
    numbers: &'a mut expression::State,
    host: &'a mut HostState,
    resources: &'a mut Resources,
    tree: &'a mut TaskTree,
    objects: &'a mut BTreeMap<u32, Box<dyn TaskObject>>,
    running: &'a mut BTreeMap<u32, RunningTask>,
    factory: &'a mut F,
    fes_scene: Option<FesSceneLoan<'a>>,
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
            fes_scene: self.fes_scene.as_mut().map(FesSceneLoan::reborrow),
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
        trace_detail!(
            self.resources.diagnostic_trace.as_ref(),
            Message,
            "[msg] sender={} target={} data={:?}",
            self.host.task_id,
            target,
            &message[1..]
        );
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
            fes_scene: None,
            budget: self.budget,
            limit: self.limit,
        }
    }
}

impl<F: ObjectFactory> Lifecycle for TaskLifecycle<'_, F> {
    fn begin_update(&mut self, id: u32) -> Result<()> {
        self.objects
            .get_mut(&id)
            .ok_or_else(|| Error::new(0, "任务对象不存在"))?
            .begin_update();
        Ok(())
    }

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

    /// 销毁完成后向父级发送子任务结束通知；普通对象把它转为等待消息。
    fn notify_child_finished(&mut self, tree: &mut TaskTree, parent: u32, id: u32) -> Result<()> {
        let kind = tree.node(parent).map(|node| node.kind).unwrap_or(0);
        let mut propagate = true;
        let mut output = 0;
        run_nested(
            self.store(tree),
            parent,
            Execution::Command {
                kind,
                command: &Command {
                    target: parent,
                    sender: id,
                    code: 20,
                    data: [0; 3],
                },
                buffers: &mut CommandBuffers::default(),
                output: &mut output,
                propagate: &mut propagate,
            },
        )
    }

    fn release(&mut self, id: u32) -> Result<()> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or_else(|| Error::new(0, "任务对象不存在"))?;
        object.release()?;
        self.resources.release_plane_movies(self.objects, id);
        self.resources.images.release_resources(id)?;
        self.resources.images.release_shared_image_owner(id)?;
        self.resources.images.release_owner(id);
        self.resources.raw.release_owner(id);
        self.resources.compositor.drawables.remove(id);
        self.resources.emocon.remove(&id);
        self.resources.legacy_cst.remove(&id);
        self.numbers.release_local(id);
        self.objects.remove(&id);
        Ok(())
    }

    fn prepare_release(&mut self, tree: &mut TaskTree, id: u32) -> Result<()> {
        run_nested(self.store(tree), id, Execution::PrepareRelease)
    }
}
