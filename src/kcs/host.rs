use super::{Host, Interpreter, MemoryCopyMode, NativeReturn, ScriptRandom, TaskClock};
use crate::task::TaskTree;
use crate::{Error, HandleAllocator, Result, expression};
use std::collections::BTreeMap;

pub(crate) fn hierarchy_query(
    tree: &TaskTree,
    task_id: u32,
    index: u32,
    argument: u32,
    pc: usize,
) -> Result<NativeReturn> {
    let node = tree
        .node(task_id)
        .ok_or_else(|| Error::new(pc, "KCS 当前任务未登记"))?;
    let id = if index == 75 {
        tree.descendants(node.id)
            .unwrap()
            .nth(argument as usize)
            .unwrap_or(u32::MAX)
    } else {
        node.parent
            .ok_or_else(|| Error::new(pc, "KCS 当前任务没有父任务"))?
    };
    Ok(NativeReturn::Value(id))
}

/// 交给窗口宿主的异步菜单项状态请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItemRequest {
    pub task_id: u32,
    pub command_id: u32,
    /// 非零启用，零禁用；保留原始 DWORD。
    pub value: u32,
}

/// 普通 9 提交的窗口关闭请求；由平台宿主消费。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCloseRequest {
    pub task_id: u32,
    /// 零参数使用应用退出通知，非零参数使用标准关闭消息。
    pub notify: bool,
}

/// 普通任务表的持久宿主状态；数值区由 `CoreHost::state` 单独提供，
/// 以便同一份状态在多次 native 调用之间保留集合句柄、随机序列和请求队列。
/// 普通 869～875 的文本记录对象；0x5C 字节内部布局与后续命令语义待原入口核实。
#[derive(Default)]
pub(super) struct TextRecord {
    /// 869 装入的四个格式展开字符串。
    pub strings: [Vec<u8>; 4],
    /// 873 装入的原始记录数据。
    pub payload: Vec<u8>,
    /// 870／872／874／875 每次调用的编号及原始参数字节。
    pub calls: Vec<(u32, Vec<u8>)>,
}

#[derive(Default)]
pub struct HostState {
    pub configured_shared_slots: u32,
    pub debug_messages: Vec<Vec<u8>>,
    /// 普通 1040 输出的 UTF-16 调试文字，包含末尾换行。
    pub unicode_debug_messages: Vec<Vec<u16>>,
    pub menu_item_requests: Vec<MenuItemRequest>,
    pub window_close_requests: Vec<WindowCloseRequest>,
    pub task_time: u32,
    pub task_origin: u32,
    pub task_id: u32,
    pub system_time_ms: u32,
    pub debug_mode: u32,
    /// 非零时将名称表的 CP932 主姓名转换为 UTF-8；附加文本保持字节编码。
    pub unicode_text: u32,
    /// 新声音分类配置的启用字段；与声音管理器或设备是否存在分别保存。
    pub sound_config_enabled: u32,
    pub sound_config: Option<crate::audio::SoundConfig>,
    pub sound_groups: Option<crate::audio::SoundGroups>,
    pub sound_settings_dirty: u32,
    /// SOUND/stereo 显式覆盖值；None 读取平台配置，未配置平台时取一。
    pub sound_stereo: Option<u32>,
    pub sequence_name_mode: u32,
    pub surface_size: [u32; 2],
    /// 窗口客户区内的绘图矩形；未配置时使用从零开始的 surface_size。
    pub window_content_rect: Option<crate::graphics::Rect>,
    /// 外层图形宿主维护的连续截图开关，保留原始 DWORD。
    pub frame_capture_enabled: u32,
    /// 下一次连续截图使用的序号；由外层捕获流程同步。
    pub frame_capture_index: u32,
    /// 消息记录生成显示时间时使用的全局比例；None 表示尚无时间上下文。
    pub message_time_scale: Option<u32>,
    pub message_defaults: Option<crate::message::MessageDefaults>,
    pub message_measurement_parameter: u32,
    pub fes_message_frame_delay: i32,
    pub system_settings: Option<crate::settings::SystemSettings>,
    pub platform_settings: Option<crate::settings::PlatformSettings>,
    /// 平台当前窗口模式的原始 DWORD；独立于保存的全屏选项。
    pub windowed_mode: Option<u32>,
    /// 独立宿主的按键配置；运行时优先读取输入设备的当前配置。
    pub configuration_keys: Option<crate::input::DeviceKeys>,
    pub configuration_services: Option<Box<dyn crate::settings::ConfigurationServices>>,
    /// 全局配置变更标记；宿主处理配置变更通知后清除。
    pub system_settings_dirty: u32,
    pub languages: crate::language::LanguageState,
    pub language_change_requests: Vec<crate::language::LanguageChangeRequest>,
    pub string_table_config: Option<crate::language::StringTableConfig>,
    pub string_tables: crate::language::StringTables,
    pub script_replacements: BTreeMap<Vec<u8>, Vec<u8>>,
    pub save_services: Option<Box<dyn crate::save::SaveServices>>,
    /// 普通 629 使用的同步平台打开服务；未配置时该入口返回诊断。
    pub system_services: Option<Box<dyn super::SystemServices>>,
    /// 外层输入宿主提供的当前逻辑画布坐标；保留画布外的值。
    pub pointer_position: [i32; 2],
    pub memory_copy_mode: MemoryCopyMode,
    /// 普通矩阵接口在地址解析失败时使用的共享矩阵，初值为零。
    pub implicit_matrix: [u32; 16],
    pub random: ScriptRandom,
    pub(super) integer_maps: BTreeMap<u32, BTreeMap<Vec<u8>, u32>>,
    pub(super) sequence_maps: BTreeMap<u32, super::sequences::SequenceMap>,
    pub(super) record_stacks: BTreeMap<u32, super::record_stack::RecordStack>,
    pub(super) containers: BTreeMap<u32, crate::save::DataContainer>,
    /// 普通 869 创建的四字符串记录；原入口装入 0x5C 字节对象。
    /// 873 装入原始数据，870／872／874／875 的调用仅记录参数，副作用待原入口核实。
    pub(super) text_records: BTreeMap<u32, TextRecord>,
    pub(super) raw_buffers: BTreeMap<u32, Vec<u8>>,
    handles: HandleAllocator,
}

impl HostState {
    pub(crate) fn sound_stereo_value(&self) -> u32 {
        self.sound_stereo.unwrap_or_else(|| {
            self.platform_settings
                .as_ref()
                .map_or(1, |settings| settings.sound_stereo)
        })
    }

    /// 458 装载的文件数据；集合清空保留数据，420 释放时失效。
    pub fn sequence_data(&self, handle: u32) -> Result<&super::SequenceData> {
        self.sequence_maps
            .get(&handle)
            .map(|map| &map.data)
            .ok_or_else(|| Error::new(0, "KCS 序列对象句柄无效"))
    }

    pub(crate) fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.sequence_maps
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "KCS 序列对象句柄无效"))?
            .load(bytes)
    }

    pub fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        if self.sound_config_enabled == 0 {
            return Ok(crate::audio::SoundGroups::legacy_id(name));
        }
        self.sound_groups
            .as_ref()
            .map(|groups| groups.id(name))
            .ok_or_else(|| Error::new(0, "宿主未提供新声音组配置"))
    }

    /// 创建空宿主状态，并与外部资源存储共用句柄编号源。
    pub fn with_handles(handles: HandleAllocator) -> Self {
        Self {
            handles,
            ..Self::default()
        }
    }

    pub fn handle_allocator(&self) -> HandleAllocator {
        self.handles.clone()
    }

    /// 借用脚本创建或装载的数据容器，存活至 246 释放或宿主销毁。
    pub fn data_container(&self, handle: u32) -> Result<&crate::save::DataContainer> {
        self.containers
            .get(&handle)
            .ok_or_else(|| Error::new(0, "KCS 数据容器句柄无效"))
    }

    pub(crate) fn data_container_mut(
        &mut self,
        handle: u32,
    ) -> Result<&mut crate::save::DataContainer> {
        self.containers
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "KCS 数据容器句柄无效"))
    }

    /// 保存任务时钟供 native 5 查询；不修改平台或其他状态。
    pub fn set_task_clock(&mut self, clock: &TaskClock) {
        self.task_origin = clock.origin;
        self.task_time = clock.time;
    }
}

/// 在已有数值状态上提供普通任务表中已核实的接口。
pub struct CoreHost<'a> {
    pub state: &'a mut expression::State,
    /// 与资源包装组合后，整体替换状态须沿用原状态的 `handle_allocator()`。
    pub host: HostState,
    pub task_tree: Option<&'a TaskTree>,
}

impl<'a> CoreHost<'a> {
    pub fn new(state: &'a mut expression::State) -> Self {
        let configured_shared_slots = state.shared.len() as u32;
        Self {
            state,
            host: HostState {
                configured_shared_slots,
                ..HostState::default()
            },
            task_tree: None,
        }
    }

    pub fn set_task_clock(&mut self, clock: &TaskClock) {
        self.host.set_task_clock(clock);
    }
}

impl std::ops::Deref for CoreHost<'_> {
    type Target = HostState;

    fn deref(&self) -> &HostState {
        &self.host
    }
}

impl std::ops::DerefMut for CoreHost<'_> {
    fn deref_mut(&mut self) -> &mut HostState {
        &mut self.host
    }
}

impl Host for CoreHost<'_> {
    fn language_selection(&self) -> [u32; 3] {
        self.host.languages.selection()
    }

    fn unicode_text(&self) -> bool {
        self.host.unicode_text != 0
    }
    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.host.load_sequence_data(handle, bytes)
    }
    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        self.host
            .message_defaults
            .as_ref()
            .ok_or_else(|| Error::new(0, "宿主未配置消息默认设置"))
    }

    fn message_time_scale(&self) -> Option<u32> {
        self.host.message_time_scale
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

    fn surface_size(&self) -> [u32; 2] {
        self.host.surface_size
    }

    fn parent_task_id(&self) -> Result<u32> {
        self.task_tree
            .and_then(|tree| tree.node(self.host.task_id))
            .and_then(|node| node.parent)
            .ok_or_else(|| Error::new(0, "当前任务没有可查询的父级"))
    }

    fn handle_allocator(&self) -> Option<HandleAllocator> {
        Some(self.host.handle_allocator())
    }

    fn set_task_id(&mut self, id: u32) {
        self.host.task_id = id;
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        CoreHost::set_task_clock(self, clock);
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        super::call_native(
            self.state,
            &mut self.host,
            self.task_tree,
            index,
            arguments,
            vm,
        )
    }
}
