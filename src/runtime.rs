//! 任务树、对象存储与普通脚本宿主的整合。
//!
//! 本模块把任务索引、基类控制、生命周期回调和真实对象存储连接起来，替代此前只能逐项手工搭建的适配层。
//! 对象类型由 [`ObjectFactory`] 创建；提供普通 KCS、基类及 [`crate::graphics::GraphicsFactory`] 的二维与图像列表对象。
//! 其他类型需要对应子系统按 [`TaskObject`] 协议实现，缺失时创建请求返回诊断。
//!
//! 同步消息、删除通知和对象命令在返回前完成预算续跑。普通对象执行期间仍接收删除标记与排队消息，
//! 向执行中普通 VM 同步发送消息返回零。输入同步回调开放同一份区域、键和基类命令状态，图形通知开放响应槽及几何查询；
//! 其他需要直接借用执行中对象的操作返回诊断。

use crate::kcs::{Host, HostState, Interpreter, NativeReturn, OrdinaryTask, Program, TaskEffects};
use crate::task::{Command, CommandBuffers, Control, Lifecycle, TaskClock, TaskTree};
use crate::{Error, Result, expression, read};
use std::collections::{BTreeMap, BTreeSet};

mod audio;
mod backlog;
mod cst;
mod emocon;
pub mod fes;
pub use cst::CstContext;
mod graphics;
mod input;
mod message;
mod motion;
mod movie;
mod native;
mod particle;
pub use motion::MotionSnapshot;
mod image_resources;
mod object;
mod ordinary;
mod resources;
mod root;
mod save;
mod transition;

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
    pub const INPUT: u32 = 11;
    pub const ORDINARY: u32 = 12;
    pub const SOUND: u32 = 13;
    pub const IMAGE_LIST: u32 = 15;
    pub const PARTICLE_3D: u32 = 16;
    pub const WIPE: u32 = 26;
    pub const FADE: u32 = 27;
    pub const WIPE_ALPHA: u32 = 28;
    pub const FES: u32 = 34;
    pub const RWIPE: u32 = 39;
    pub const SCROLL2: u32 = 41;
    pub const SLIDER: u32 = 42;
    pub const STRING_PLANE: u32 = 48;
    pub const RENDER_WMOSAIC: u32 = 50;
    pub const RENDER_FMOSAIC: u32 = 51;
    pub const STRING_PLANE_UTF8: u32 = 63;
    pub const CAMERA_2D: u32 = 65;
    /// 粒子任务（`kcTaskParticle`）；控制属性已接入，装载、播放及绘制待接入。
    pub const PARTICLE: u32 = 38;
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

/// 只创建普通 KCS 对象的工厂；其他类型由子系统提供的工厂接入。
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
        if kind != kind::ORDINARY {
            return Err(Error::new(0, format!("对象类型 {kind} 需要对应子系统工厂")));
        }
        let program = self
            .programs
            .load(name)?
            .ok_or_else(|| Error::new(0, "KCS 程序不存在"))?;
        let mut task = OrdinaryTask::new(program, Vec::new())?;
        task.interpreter.bind_external(self.external.clone());
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
    configuration_notification: Option<BTreeSet<u32>>,
}

impl<F: ObjectFactory> Runtime<F> {
    /// 以已经创建好的根对象建立任务树；根对象编号用于后续自动编号与查询。
    pub fn new(
        root_id: u32,
        root_kind: u32,
        mut root: Box<dyn TaskObject>,
        numbers: expression::State,
        factory: F,
    ) -> Self {
        let mut host = HostState::default();
        host.configured_shared_slots = numbers.shared.len() as u32;
        let resources = Resources::with_handles(host.handle_allocator());
        root.initialize_language(host.languages.selection());
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
        if let Some(frame) = self.resources.movie_frame(output.size(), filter)? {
            return frame.render_into(output, self.resources.compositor.drawing.target.as_ref());
        }
        self.resources
            .compositor
            .render_frame(output, projection, parent, filter)
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

    /// 连接窗口呈现及脚本画面捕获共用的后端；未配置时捕获继续使用软件绘制。
    /// 后端捕获失败时保留前次完整目标。
    pub fn set_graphics_backend(
        &mut self,
        backend: impl crate::graphics::GraphicsBackend + 'static,
    ) {
        self.resources.graphics_backend = Some(Box::new(backend));
    }

    /// 以已配置的帧视图和当前画布合成绘制列表。
    pub fn compose_frame(&mut self) -> Result<crate::graphics::DrawList> {
        self.resources.compose_frame(None)
    }

    /// 通过已连接的图形后端呈现完整帧；暂时无法呈现时返回 false。
    pub fn present_frame(&mut self, size: [u32; 2]) -> Result<bool> {
        self.resources.present_frame(size)
    }

    /// 设置内部捕获所读取的画面区域；None 使用 surface_size 定义的整幅画面。
    pub fn set_frame_capture_rectangle(&mut self, rectangle: Option<crate::graphics::Rect>) {
        self.resources.capture_rectangle = rectangle;
    }

    /// 最近一次离屏绘制的目标；绘制错误保留已写像素，跳过的调用保留旧目标。
    pub fn offscreen_frame(&self) -> Option<&crate::graphics::RenderTarget> {
        self.resources.offscreen_frame.as_ref()
    }

    /// 配置同步资源读取与类别名称补全；已有资源对象继续保留。
    pub fn set_resources(&mut self, services: impl crate::kcs::ResourceServices + 'static) {
        self.resources.services = Some(Box::new(crate::kcs::CachedResources::new(services)));
    }

    /// 处理平台语言请求的字符串表部分；请求队列和平台控件由宿主管理。
    pub fn reload_language_tables(
        &mut self,
        target: crate::language::LanguageTarget,
    ) -> Result<()> {
        if target == crate::language::LanguageTarget::Subtitle {
            return Ok(());
        }
        let config = self
            .host
            .string_table_config
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置字符串表路径"))?;
        config.reload(
            target,
            &self.host.languages,
            &mut self.host.string_tables,
            |path| {
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置字符串表资源服务"))?;
                if let Some(bytes) =
                    services.read_utf16(path, crate::resource::ResourceMode::File)?
                {
                    return Ok(Some(bytes));
                }
                services.read_utf16(path, crate::resource::ResourceMode::LooseFirst)
            },
        )
    }

    /// 按当前选择处理语言请求的资源、编号及变更标记；队列和平台控件由宿主管理。
    pub fn refresh_language(&mut self, target: crate::language::LanguageTarget) -> Result<()> {
        use crate::language::LanguageTarget;
        let count = self.host.languages.prefixes.len();
        if target == LanguageTarget::Interface
            && self.host.languages.primary_index as usize >= count
        {
            return Ok(());
        }
        self.reload_language_tables(target)?;
        let mark_changed = match target {
            LanguageTarget::Interface => true,
            LanguageTarget::MainText => (self.host.languages.message_index as usize) < count,
            LanguageTarget::Subtitle => {
                if self.host.languages.subtitle_index as usize >= count {
                    self.host.languages.subtitle_index = u32::MAX;
                }
                true
            }
        };
        if mark_changed {
            self.host.system_settings_dirty = 1;
        }
        Ok(())
    }

    /// 配置普通 629 的同步系统打开服务。
    pub fn set_system_services(&mut self, services: impl crate::kcs::SystemServices + 'static) {
        self.host.system_services = Some(Box::new(services));
    }

    pub fn set_configuration_services(
        &mut self,
        services: impl crate::settings::ConfigurationServices + 'static,
    ) {
        self.host.configuration_services = Some(Box::new(services));
    }

    /// 配置按任务存档目录读取 CSS 文件及查询修改时间的服务；已有容器继续保留。
    pub fn set_save_services(&mut self, services: impl crate::save::SaveServices + 'static) {
        self.host.save_services = Some(Box::new(services));
    }

    /// 配置消息对象使用的字体选择、度量与灰度字形服务。
    pub fn set_fonts(&mut self, fonts: impl crate::message::FontServices + 'static) {
        self.resources.fonts = Some(Box::new(fonts));
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

    /// 365 装载的原始图像字节，存活至 366 显式释放或运行时销毁。
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

    /// 完成一轮树更新后广播配置变更并清除标记。
    /// 通知失败后先续完原轮次，不重复推进时钟和主体。
    pub fn update(&mut self, now: u32, increment: u32) -> Result<bool> {
        if let Some(movie) = &mut self.resources.movie {
            movie.advance(self.host.system_time_ms)?;
        }
        if self.configuration_notification.is_some() {
            return self.finish_configuration_notification();
        }
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
        let active = self.tree.update(now, increment, &mut lifecycle)?;
        if active && self.host.system_settings_dirty != 0 {
            self.configuration_notification = Some(BTreeSet::new());
            return self.finish_configuration_notification();
        }
        Ok(active)
    }

    fn finish_configuration_notification(&mut self) -> Result<bool> {
        let mut pending = self.configuration_notification.take().unwrap();
        let root = self.tree.root_id();
        if self.tree.node(root).is_none() {
            return Ok(false);
        }
        let result = route_command_mode(
            self.store(),
            root,
            Command {
                target: root,
                sender: 0,
                code: 19,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut 0,
            crate::input::InputDelivery::Broadcast,
            Some(&mut pending),
        );
        if let Err(error) = result {
            self.configuration_notification = Some(pending);
            return Err(error);
        }
        self.host.system_settings_dirty = 0;
        Ok(true)
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
    } else {
        factory.create(id, kind, name)?
    };
    object.initialize_language(host.languages.selection());
    object.set_creation_context(context)?;
    if let (Some(audio), Some(groups)) = (object.audio_state_mut(), host.sound_groups.as_ref()) {
        audio.set_groups(groups)?;
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
        if *kind == kind::PLANE && matches!(command.code, 61 | 62) {
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
    }
    if let Some(task) = running.get_mut(&target) {
        if let Execution::Command {
            command, propagate, ..
        } = &mut execution
        {
            if command.code == 29 {
                if !task.deleted {
                    task.clock.paused = command.data[1] as i32;
                    task.paused_changed = Some(command.data[1] as i32);
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
                        output,
                    )?;
                }
                return Ok(());
            }
            if matches!(command.code, 342 | 346 | 347) {
                **propagate = false;
                if !running.get(&target).is_some_and(|task| task.deleted) {
                    let scene = &mut fes_scene.as_mut().unwrap().scene;
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
        if std::env::var_os("KATZE_TRACE_MSG").is_some() {
            eprintln!(
                "[msg] sender={} target={} data={:?}",
                self.host.task_id,
                target,
                [message[1], message[2], message[3]]
            );
        }
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
            67 | 75 | 109 | 272..=274 | 452 | 520 => 4,
            108 | 110 | 275 | 426 | 427 | 528 | 531 | 694 => 8,
            69 | 70 | 72 | 73 | 527 => 16,
            530 | 905 => 20,
            76 => 0,
            _ => unreachable!(),
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 5];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        match index {
            272 | 273 => {
                self.command_target(args[0], index + 49, [0; 3])?;
                Ok(NativeReturn::Continue)
            }
            274 => Ok(NativeReturn::Value(
                self.command_target(args[0], 324, [0; 3])?,
            )),
            275 => {
                let mut parameters = vm.read(args[1], 128).ok().map(|bytes| bytes.to_vec());
                self.command_target_with_buffers(
                    args[0],
                    325,
                    [0, args[1], 0],
                    &mut CommandBuffers {
                        data1: parameters.as_deref_mut(),
                        ..CommandBuffers::default()
                    },
                )?;
                Ok(NativeReturn::Continue)
            }
            426 | 427 => {
                self.command_target(args[0], index - 100, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            452 => {
                let sender = self.host.task_id;
                if self.require_reachable(args[0])
                    && self
                        .tree
                        .node(args[0])
                        .is_some_and(|node| node.kind == kind::ORDINARY)
                {
                    if let Some(task) = self.running.get(&args[0]) {
                        return Ok(NativeReturn::Value(if task.deleted {
                            u32::MAX
                        } else {
                            task.property
                        }));
                    }
                }
                let mut output = u32::MAX;
                route_command(
                    self.store(),
                    sender,
                    Command {
                        target: args[0],
                        sender,
                        code: 23,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                    &mut output,
                )?;
                Ok(NativeReturn::Value(output))
            }
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
                let context = self.context[..end].to_vec();
                let id = create_object(self.store(), args[1], args[0], &name, &context)?;
                Ok(NativeReturn::Value(id))
            }
            67 => {
                if !self.require_reachable(args[0]) {
                    return Err(Error::new(vm.pc, "任务删除目标不存在"));
                }
                if self.resources.input_callbacks.contains_key(&args[0]) {
                    run_nested(self.store(), args[0], Execution::Delete)?;
                    return Ok(NativeReturn::Continue);
                }
                if let Some(task) = self.running.get_mut(&args[0]) {
                    if task.remaining_messages.is_none() && !task.deferred_delete {
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
                    let owner = self.host.task_id;
                    self.running.get_mut(&owner).unwrap().memory = Some(vm.lend_memory());
                    let result = self.message_target(args[0], &mut message);
                    vm.restore_memory(self.running.get_mut(&owner).unwrap().memory.take().unwrap());
                    result?
                };
                Ok(NativeReturn::Value(value))
            }
            109 => Ok(NativeReturn::Value(
                self.command_target(args[0], 96, [0; 3])?,
            )),
            // 遮罩划扫任务协议：520 查询运行状态，527/528 写入补充参数块，530/905 写入主
            // 配置（末位标记区分两个入口），531 设置运行状态。
            520 => Ok(NativeReturn::Value(if args[0] == 0 {
                0
            } else {
                self.command_target(args[0], 383, [0; 3])?
            })),
            527 => {
                if args[0] != 0 {
                    let mut block = arguments[4..16].to_vec();
                    self.command_target_with_buffers(
                        args[0],
                        390,
                        [0; 3],
                        &mut CommandBuffers {
                            data1: Some(&mut block),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(NativeReturn::Continue)
            }
            528 => {
                if args[0] != 0 {
                    self.command_target(args[0], 391, [0, args[1], 0])?;
                }
                Ok(NativeReturn::Continue)
            }
            530 | 905 => {
                if args[0] != 0 && args[4] != 0 {
                    // 原入口把 args[4] 当作指针解引用进配置块；此处按可解析的 VM 地址
                    // 读取，否则保留其原始位形。
                    let extra = vm
                        .read(args[4], 4)
                        .map(|bytes| read::u32_at(&bytes, 0))
                        .unwrap_or_else(|_| Ok(args[4]))?;
                    let mut block: Vec<u8> = arguments[8..16].to_vec();
                    block.extend_from_slice(&arguments[4..8]);
                    block.extend_from_slice(&extra.to_le_bytes());
                    block.extend_from_slice(&u32::from(index == 905).to_le_bytes());
                    self.command_target_with_buffers(
                        args[0],
                        393,
                        [0; 3],
                        &mut CommandBuffers {
                            data1: Some(&mut block),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(NativeReturn::Continue)
            }
            531 => {
                if args[0] != 0 {
                    self.command_target(args[0], 394, [0, args[1], 0])?;
                }
                Ok(NativeReturn::Continue)
            }
            108 | 110 => {
                self.command_target(args[0], if index == 108 { 95 } else { 97 }, [0, args[1], 0])?;
                Ok(NativeReturn::Continue)
            }
            694 => {
                self.command_target(args[0], 29, [0, args[1], 0])?;
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
                    if let Some(loan) = self
                        .running
                        .get_mut(&args[0])
                        .and_then(|task| task.memory.as_mut())
                    {
                        crate::kcs::copy_loan(
                            vm,
                            loan,
                            args[1],
                            args[3],
                            count,
                            self.host.memory_copy_mode,
                            index == 73,
                        )?;
                        return Ok(NativeReturn::Continue);
                    }
                    let other = self
                        .objects
                        .get_mut(&args[0])
                        .and_then(|object| object.interpreter_mut())
                        .ok_or_else(|| Error::new(vm.pc, "任务对象不存在、正在执行或没有 VM"))?;
                    if index == 72 {
                        crate::kcs::copy_memory_between(
                            vm,
                            args[3],
                            other,
                            args[1],
                            count,
                            self.host.memory_copy_mode,
                        )?;
                    } else {
                        crate::kcs::copy_memory_between(
                            other,
                            args[1],
                            vm,
                            args[3],
                            count,
                            self.host.memory_copy_mode,
                        )?;
                    }
                }
                Ok(NativeReturn::Continue)
            }
            _ => unreachable!(),
        }
    }
}

impl<F: ObjectFactory> Host for ObjectHost<'_, F> {
    fn update_particle(
        &mut self,
        task: &mut crate::kcs::ParticleTask,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        self.update_particle_task(task, now, budget)
    }

    fn language_selection(&self) -> [u32; 3] {
        self.host.languages.selection()
    }

    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.host.load_sequence_data(handle, bytes)
    }
    fn set_task_shared_numbers(&mut self, available: bool) {
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.shared_numbers = Some(available);
        }
    }
    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<Program>> {
        self.resources.images.release_resources(self.host.task_id)?;
        self.resources.images.release_owner(self.host.task_id);
        self.resources.raw.release_owner(self.host.task_id);
        self.factory.load_kcs_program(name)
    }

    fn fes_services(&mut self) -> Option<&mut dyn fes::Services> {
        Some(self)
    }

    fn surface_size(&self) -> [u32; 2] {
        self.host.surface_size
    }

    fn parent_task_id(&self) -> Result<u32> {
        self.tree
            .node(self.host.task_id)
            .and_then(|node| node.parent)
            .ok_or_else(|| Error::new(0, "当前任务没有可查询的父级"))
    }

    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        Some(self.host.handle_allocator())
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.resources.images.get(handle)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.resources.raw.image_data(handle)
    }

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        Some(&mut self.resources.images)
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::message::FontServices> {
        self.resources
            .fonts
            .as_mut()
            .map(|fonts| fonts.as_mut() as _)
    }

    fn message_time_scale(&self) -> Option<u32> {
        self.host.message_time_scale
    }

    fn unicode_text(&self) -> bool {
        self.host.unicode_text != 0
    }

    fn sound_stereo(&self) -> u32 {
        self.host.sound_stereo_value()
    }

    fn sound_config_enabled(&self) -> u32 {
        self.host.sound_config_enabled
    }

    fn jtalk_enabled(&self) -> u32 {
        self.host
            .system_settings
            .as_ref()
            .map_or(0, |settings| settings.jtalk_enabled)
    }

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        self.host.sound_group_id(name)
    }

    fn system_time_ms(&self) -> u32 {
        self.host.system_time_ms
    }

    fn effect_speed(&self) -> u32 {
        self.host.system_settings.as_ref().map_or(0, |settings| {
            u32::from_le_bytes(settings.effects[..4].try_into().unwrap())
        })
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        self.host
            .message_defaults
            .as_ref()
            .ok_or_else(|| Error::new(0, "宿主未配置消息默认设置"))
    }

    fn message_plane_display(&mut self, id: u32) -> Result<Option<u32>> {
        let drawing = &self.resources.compositor.drawing;
        if drawing.mode != 0 || !drawing.enabled {
            return Ok(None);
        }
        Ok(self
            .resources
            .compositor
            .drawables
            .get(id)
            .map(crate::graphics::Drawable::display))
    }

    fn submit_message(
        &mut self,
        surface: &crate::graphics::Surface,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        Ok(self
            .resources
            .compositor
            .drawing
            .copy_surface(surface, rectangle, at))
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
        clip: Option<crate::graphics::Rect>,
    ) -> Result<bool> {
        self.resources.update_image_list(list, submit, force, clip)
    }

    fn update_image_list_animations(
        &mut self,
        list: &mut crate::graphics::ImageList,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        list.update_animations_with_random(now, budget, &mut self.resources.images, &mut || {
            self.host.random.next_integer()
        })
    }

    fn restart_image_list_animation(
        &mut self,
        list: &mut crate::graphics::ImageList,
        key: u32,
        budget: usize,
    ) -> Result<bool> {
        list.restart_animation_with_random(key, budget, &mut self.resources.images, &mut || {
            self.host.random.next_integer()
        })
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        let parent = self.host.task_id;
        create_object(self.store(), parent, kind, name, context)
    }
    fn input_device(&mut self) -> Option<&mut dyn crate::input::InputDevice> {
        self.resources
            .input_device
            .as_mut()
            .map(|device| device.as_mut() as _)
    }

    fn input_coordinates(&self) -> crate::input::CoordinateMap {
        self.resources.input_coordinates
    }
    fn input_pointer_control(&self) -> bool {
        self.resources.input_pointer_control
    }
    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.resources
            .compositor
            .drawing
            .target
            .as_ref()
            .map(|t| crate::graphics::CanvasLayout::new(t.tile_size(), t.grid(), t.tile_size()))
            .transpose()
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        Some(&mut self.resources.compositor.drawables)
    }

    fn compositor(&mut self) -> Option<&mut crate::graphics::Compositor> {
        Some(&mut self.resources.compositor)
    }

    fn reserve_auxiliary_targets(&mut self, count: u32) -> Result<bool> {
        let settings = self.host.platform_settings.as_ref();
        if settings.is_some_and(|settings| settings.d3d_backbuffer == 0) {
            return Ok(false);
        }
        self.resources.compositor.reserve_auxiliary_targets(
            self.host.surface_size,
            count,
            settings.map_or(u32::MAX, |settings| settings.d3d_tex_size),
        )?;
        Ok(count <= 2 && self.resources.compositor.auxiliary_targets.len() >= count as usize)
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

    fn dispatch_input_command(
        &mut self,
        state: &mut crate::input::CommandState,
        delivery: crate::input::InputDelivery,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.input_callbacks.contains_key(&origin)
        {
            return Err(Error::new(0, "输入回调上下文不可用"));
        }
        self.resources
            .input_callbacks
            .insert(origin, std::mem::take(state));
        let result = route_command_mode(
            self.store(),
            origin,
            command,
            buffers,
            output,
            delivery,
            None,
        );
        *state = self.resources.input_callbacks.remove(&origin).unwrap();
        result
    }

    fn dispatch_graphics_event(
        &mut self,
        response: &mut u32,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.graphics_responses.contains_key(&origin)
        {
            return Err(Error::new(0, "图形回调上下文不可用"));
        }
        self.resources.graphics_responses.insert(origin, *response);
        let result = route_command(self.store(), origin, command, buffers, output);
        *response = self.resources.graphics_responses.remove(&origin).unwrap();
        result
    }

    fn dispatch_button_event(
        &mut self,
        button: &mut crate::input::ButtonObject,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.button_callbacks.contains_key(&origin)
        {
            return Err(Error::new(0, "按钮回调上下文不可用"));
        }
        self.resources
            .button_callbacks
            .insert(origin, button.take_for_callback());
        let result = route_command(self.store(), origin, command, buffers, output);
        *button = self.resources.button_callbacks.remove(&origin).unwrap();
        result
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> TaskEffects {
        let Some(task) = self.running.get_mut(&self.host.task_id) else {
            return TaskEffects::default();
        };
        let effects = std::mem::take(&mut task.effects);
        if let Some(remaining) = &mut task.remaining_messages {
            *remaining = read::MAX_ITEMS
                .saturating_sub(queued_messages.saturating_add(effects.messages.len()));
        }
        effects
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.host.set_task_clock(clock);
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.clock = clock.clone();
            if let Some(paused) = task.paused_changed {
                task.clock.paused = paused;
            }
        }
    }

    fn set_task_id(&mut self, id: u32) {
        self.host.task_id = id;
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.context.clear();
        self.context.extend_from_slice(context);
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.context = Some(context.to_vec());
        }
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        self.call_registered_native(index, arguments, vm)
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
        self.resources.images.release_resources(id)?;
        self.resources.images.release_owner(id);
        self.resources.raw.release_owner(id);
        self.resources.compositor.drawables.remove(id);
        self.resources.emocon.remove(&id);
        self.objects.remove(&id);
        Ok(())
    }

    fn prepare_release(&mut self, tree: &mut TaskTree, id: u32) -> Result<()> {
        run_nested(self.store(tree), id, Execution::PrepareRelease)
    }
}
