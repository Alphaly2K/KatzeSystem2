pub use crate::memory::{MemoryRead, SharedMemory};
use crate::{Error, Result, Step, read};
use std::collections::BTreeMap;

mod configuration;
mod data;
mod debug;
mod host;
mod map_data;
mod matrix;
mod memory;
mod message_text;
mod messages;
mod motion;
mod name_table;
mod numeric;
mod ordinary;
mod particle;
mod queue;
mod random;
mod record_stack;
mod resource_cache;
mod resource_host;
mod sequences;
mod sse;
mod strings;
mod system_host;
mod task_host;
mod triples;
mod utf8;
mod vectors;
pub use crate::task::TaskClock;
pub(crate) use configuration::save_configuration;
pub use host::{CoreHost, HostState, MenuItemRequest, WindowCloseRequest};
pub(crate) use host::{call_native, hierarchy_query};
pub(crate) use matrix::{multiply as multiply_matrix, transform as transform_vector};
pub use memory::MemoryCopyMode;
pub(crate) use memory::copy as copy_memory;
pub(crate) use memory::copy_between as copy_memory_between;
pub(crate) use memory::{MemoryLoan, copy_loan};
pub use motion::{MotionHost, MotionState, MotionTask};
pub(crate) use numeric::float as float_operation;
pub use ordinary::OrdinaryTask;
pub use particle::{ParticleHost, ParticleSource, ParticleTask, ParticleVertex};
pub use queue::MotionQueue;
pub use random::ScriptRandom;
pub(crate) use resource_cache::call as prefetch_call;
pub use resource_cache::{CachedResources, ResourceCache};
pub(crate) use resource_host::ResourceState;
pub(crate) use resource_host::load_sequence_file;
pub use resource_host::{ResourceHost, ResourceServices};
pub use sequences::SequenceData;
pub(crate) use strings::format as format_string;
pub use system_host::{SystemHost, SystemServices};
pub use task_host::{TaskHost, TaskServices};

#[derive(Clone, Copy, Debug)]
pub struct Operand {
    pub flags: u8,
    pub payload: u32,
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: u8,
    pub operands: Vec<Operand>,
}

#[derive(Clone, Debug)]
pub struct Program {
    instructions: Vec<Instruction>,
    pub control: u32,
    pub reserved: u32,
    code_len: usize,
    data: Vec<u8>,
    stack_size: usize,
}

pub fn operand_count(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x00 | 0x2d | 0x33 | 0x3a..=0x3c => 0,
        0x08
        | 0x0c
        | 0x16..=0x1a
        | 0x25..=0x29
        | 0x2e..=0x2f
        | 0x31..=0x32
        | 0x36..=0x39
        | 0x3d
        | 0x42
        | 0x44 => 1,
        0x01..=0x07
        | 0x09..=0x0b
        | 0x0d..=0x15
        | 0x1b..=0x24
        | 0x2a..=0x2c
        | 0x30
        | 0x34
        | 0x3e
        | 0x41
        | 0x43 => 2,
        0x35 => 3,
        _ => return None,
    })
}

impl Program {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(0, "KCS 容器超过读取上限"));
        }
        if bytes.get(..4) != Some(b"KCS\0")
            || read::u32_at(bytes, 4)? != 2
            || read::u32_at(bytes, 12)? != 52
        {
            return Err(Error::new(0, "未支持的 KCS 容器"));
        }
        let a = read::sized(read::u32_at(bytes, 16)?, 16)?;
        let b = read::sized(read::u32_at(bytes, 28)?, 28)?;
        let n = read::u32_at(bytes, 24)? as usize;
        let d = read::sized(read::u32_at(bytes, 32)?, 32)?;
        let s = read::sized(read::u32_at(bytes, 36)?, 36)?;
        let capacity = read::sized(read::u32_at(bytes, 48)?, 48)?;
        if read::u32_at(bytes, 20)? as usize != 52 + a
            || read::u32_at(bytes, 40)? as usize != 52 + a + b
            || a + b > capacity
            || n > b / 8
            || d + s > read::MAX_BYTES
        {
            return Err(Error::new(16, "KCS 区间字段不一致"));
        }
        let body = read::inflate(
            bytes
                .get(52..)
                .ok_or_else(|| Error::new(52, "KCS 头被截断"))?,
            a + b,
        )?;
        let init = &body[a..];
        let mut data = vec![0; d];
        for i in 0..n {
            let source = read::u32_at(init, i * 8)? as usize;
            let target = read::u32_at(init, i * 8 + 4)? as usize;
            if source < n * 8 {
                return Err(Error::new(i * 8, "字符串源指向初始化表"));
            }
            let string = read::zstr(init, source)?;
            let len = string.len() + 1;
            if target.checked_add(len).is_none_or(|end| end > d) {
                return Err(Error::new(i * 8 + 4, "字符串目标超过数据区"));
            }
            data[target..target + string.len()].copy_from_slice(string);
            data[target + string.len()] = 0;
        }
        let mut program = Self::from_code(&body[..a], data, s)?;
        program.control = read::u32_at(bytes, 8)?;
        program.reserved = read::u32_at(bytes, 44)?;
        Ok(program)
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn from_code(code: &[u8], data: Vec<u8>, stack_size: usize) -> Result<Self> {
        if code.len() > read::MAX_BYTES
            || data
                .len()
                .checked_add(stack_size)
                .is_none_or(|n| n > read::MAX_BYTES)
        {
            return Err(Error::new(0, "KCS 存储超过上限"));
        }
        let mut instructions = Vec::new();
        let mut p = 0;
        while p < code.len() {
            let offset = p;
            let opcode = code[p];
            p += 1;
            let count = operand_count(opcode)
                .ok_or_else(|| Error::new(offset, format!("无效 KCS opcode {opcode:02x}")))?;
            let mut operands = Vec::new();
            for _ in 0..count {
                operands.push(Operand {
                    flags: read::range(code, p, 1)?[0],
                    payload: read::u32_at(code, p + 1)?,
                });
                p += 5;
            }
            instructions.push(Instruction {
                offset,
                opcode,
                operands,
            });
            if instructions.len() > read::MAX_ITEMS {
                return Err(Error::new(offset, "KCS 指令数量超过上限"));
            }
        }
        for ins in &instructions {
            let target = match ins.opcode {
                0x27..=0x2a => Some(0),
                0x34 => Some(1),
                0x35 => Some(2),
                _ => None,
            };
            if let Some(arg) = target {
                let op = ins.operands[arg];
                if op.flags == 3
                    && op.payload as usize != code.len()
                    && instructions
                        .binary_search_by_key(&(op.payload as usize), |i| i.offset)
                        .is_err()
                {
                    return Err(Error::new(ins.offset, "KCS 静态目标不在指令边界"));
                }
            }
        }
        Ok(Self {
            instructions,
            control: 0,
            reserved: 0,
            code_len: code.len(),
            data,
            stack_size,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReturn {
    Yield,
    Value(u32),
    Continue,
    YieldValue(u32),
    YieldVoid,
    Retry,
}

/// VM 执行期间收到的普通任务操作，由任务包装层在推进返回后应用。
#[derive(Default)]
pub struct TaskEffects {
    pub delete: bool,
    pub messages: Vec<[u32; 4]>,
}

pub trait Host {
    /// 配置通知比较的界面、主文字、副文字原始选择。
    fn language_selection(&self) -> [u32; 3] {
        [0, 0, u32::MAX]
    }

    /// 名称表装载时使用的全局 Unicode 模式；默认保留 CP932 主姓名。
    fn unicode_text(&self) -> bool {
        false
    }
    /// 将已经读取的文件绑定到存活的序列集合，保留已有名称树。
    fn load_sequence_data(&mut self, _handle: u32, _bytes: Vec<u8>) -> Result<()> {
        Err(Error::new(0, "宿主未提供序列文件存储"))
    }
    fn load_kcs_program(&mut self, _name: &[u8]) -> Result<Option<Program>> {
        Err(Error::new(0, "宿主未提供 KCS 程序装载"))
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn>;

    /// 外层资源宿主共用的编号源；不创建句柄的宿主可以不提供。
    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        None
    }

    fn set_task_clock(&mut self, _clock: &TaskClock) {}

    fn set_task_id(&mut self, _id: u32) {}

    fn set_task_context(&mut self, _context: &[u8]) {}

    fn set_task_shared_numbers(&mut self, _available: bool) {}

    /// FES 任务使用同一运行时的共享状态、对象更新和立即启动服务。
    fn fes_services(&mut self) -> Option<&mut dyn crate::runtime::fes::Services> {
        None
    }

    /// 具体绘图对象使用的登记与属性服务；独立解释宿主可以不提供。
    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        None
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::message::FontServices> {
        None
    }

    fn message_time_scale(&self) -> Option<u32> {
        None
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        Err(Error::new(0, "宿主未配置消息默认设置"))
    }

    fn message_plane_display(&mut self, id: u32) -> Result<Option<u32>> {
        self.drawables()
            .map(|drawings| drawings.get(id).map(crate::graphics::Drawable::display))
            .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))
    }

    /// 将顶端起始的原始像素复制到画布，返回是否触及目标。
    fn submit_message(
        &mut self,
        _surface: &crate::graphics::Surface,
        _rectangle: crate::graphics::Rect,
        _at: [i32; 2],
    ) -> Result<bool> {
        Err(Error::new(0, "宿主未提供消息画布提交服务"))
    }

    /// 已解码图像的借用；调用方在同步命令结束前完成像素读取。
    fn image(&self, _handle: u32) -> Result<&crate::graphics::Image> {
        Err(Error::new(0, "宿主未提供解码图像查询"))
    }

    /// 原始图像数据的同步借用；来源使用者自行持有解码后的帧。
    fn image_data(&self, _handle: u32) -> Result<&[u8]> {
        Err(Error::new(0, "宿主未提供原始图像数据查询"))
    }

    /// 图像对象的可变存储；使用者持有期间必须沿用同一存储。
    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        None
    }

    /// 全局画面效果速度，缺省使用配置构造值零。
    fn effect_speed(&self) -> u32 {
        0
    }

    /// SOUND/stereo 的原始开关；缺省使用配置构造值一。
    fn sound_stereo(&self) -> u32 {
        1
    }

    /// 声音分类配置的原始启用值；零使用旧分类模式。
    fn sound_config_enabled(&self) -> u32 {
        0
    }

    /// GAME/use_jtalk 的原始开关；缺省使用配置构造值零。
    fn jtalk_enabled(&self) -> u32 {
        0
    }

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        Ok(crate::audio::SoundGroups::legacy_id(name))
    }

    fn system_time_ms(&self) -> u32 {
        0
    }

    /// 更新列表并按开关提交脏区或整幅图像；返回是否复制了画布像素。
    fn update_image_list(
        &mut self,
        _list: &mut crate::graphics::ImageList,
        _submit: bool,
        _force: bool,
    ) -> Result<bool> {
        Err(Error::new(0, "宿主未提供图像列表更新服务"))
    }

    /// 推进列表动画；完成后再绘制，本轮预算续跑保持同一时刻。
    fn update_image_list_animations(
        &mut self,
        list: &mut crate::graphics::ImageList,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        let images = self
            .images_mut()
            .ok_or_else(|| Error::new(0, "宿主未提供图像存储"))?;
        list.update_animations(now, budget, images)
    }

    fn restart_image_list_animation(
        &mut self,
        list: &mut crate::graphics::ImageList,
        key: u32,
        budget: usize,
    ) -> Result<bool> {
        let images = self
            .images_mut()
            .ok_or_else(|| Error::new(0, "宿主未提供图像存储"))?;
        list.restart_animation(key, budget, images)
    }

    /// 具体对象创建纹理包装时读取的布局，独立于当前像素访问开关。
    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        Ok(None)
    }

    /// 具体对象同步读取资源所用的服务；类别补全由请求入口决定。
    fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
        None
    }

    fn input_device(&mut self) -> Option<&mut dyn crate::input::InputDevice> {
        None
    }
    fn input_coordinates(&self) -> crate::input::CoordinateMap {
        crate::input::CoordinateMap::default()
    }
    fn input_pointer_control(&self) -> bool {
        false
    }

    /// 普通 8 使用的渲染上下文尺寸，独立于输入坐标映射与当前绘图目标。
    fn surface_size(&self) -> [u32; 2] {
        [0; 2]
    }

    /// 查询当前对象的直接父任务编号；未提供服务或没有父任务时返回诊断。
    fn parent_task_id(&self) -> Result<u32> {
        Err(Error::new(0, "宿主未提供当前任务父级查询"))
    }

    /// 同步创建直属子任务；名称及上下文为未展开的原始字节。
    fn create_task_child(&mut self, _kind: u32, _name: &[u8], _context: &[u8]) -> Result<u32> {
        Err(Error::new(0, "宿主未提供子任务创建"))
    }

    /// 查询当前对象在有符号编号游标之后的直属子任务；None 从首项开始。
    fn next_task_child(&self, _previous: Option<u32>) -> Result<Option<u32>> {
        Err(Error::new(0, "宿主未提供任务子级查询"))
    }

    fn dispatch_task_command(
        &mut self,
        _command: crate::task::Command,
        _buffers: &mut crate::task::CommandBuffers<'_>,
        _output: &mut u32,
    ) -> Result<bool> {
        Err(Error::new(0, "宿主未提供任务命令分派"))
    }

    /// 输入同步路由期间开放同一份命令状态，返回前须保留嵌套命令的修改。
    fn dispatch_input_command(
        &mut self,
        _state: &mut crate::input::CommandState,
        _delivery: crate::input::InputDelivery,
        _command: crate::task::Command,
        _buffers: &mut crate::task::CommandBuffers<'_>,
        _output: &mut u32,
    ) -> Result<bool> {
        Err(Error::new(0, "宿主未提供输入回调命令分派"))
    }

    /// 图形事件同步路由期间开放命令 95 的响应槽；错误返回也保留已完成的回写。
    fn dispatch_graphics_event(
        &mut self,
        _response: &mut u32,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        self.dispatch_task_command(command, buffers, output)
    }

    /// 矩形按钮事件期间开放同一份按钮状态，保留同步属性命令的修改。
    fn dispatch_button_event(
        &mut self,
        button: &mut crate::input::ButtonObject,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        self.dispatch_graphics_event(button.response_mut(), command, buffers, output)
    }

    /// 取走当前任务的待应用操作，并提供已保存的队列长度供宿主检查入队容量。
    fn take_task_effects(&mut self, _queued_messages: usize) -> TaskEffects {
        TaskEffects::default()
    }
}

impl<T: Host + ?Sized> Host for &mut T {
    fn language_selection(&self) -> [u32; 3] {
        (**self).language_selection()
    }

    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        (**self).load_sequence_data(handle, bytes)
    }
    fn set_task_shared_numbers(&mut self, available: bool) {
        (**self).set_task_shared_numbers(available);
    }
    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<Program>> {
        (**self).load_kcs_program(name)
    }

    fn fes_services(&mut self) -> Option<&mut dyn crate::runtime::fes::Services> {
        (**self).fes_services()
    }

    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        (**self).handle_allocator()
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        (**self).call(index, arguments, vm)
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        (**self).set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        (**self).set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        (**self).set_task_context(context);
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        (**self).drawables()
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        (**self).image(handle)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        (**self).image_data(handle)
    }

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        (**self).images_mut()
    }

    fn effect_speed(&self) -> u32 {
        (**self).effect_speed()
    }

    fn unicode_text(&self) -> bool {
        (**self).unicode_text()
    }

    fn sound_stereo(&self) -> u32 {
        (**self).sound_stereo()
    }

    fn sound_config_enabled(&self) -> u32 {
        (**self).sound_config_enabled()
    }

    fn jtalk_enabled(&self) -> u32 {
        (**self).jtalk_enabled()
    }

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        (**self).sound_group_id(name)
    }

    fn system_time_ms(&self) -> u32 {
        (**self).system_time_ms()
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::message::FontServices> {
        (**self).fonts()
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        (**self).message_defaults()
    }

    fn message_time_scale(&self) -> Option<u32> {
        (**self).message_time_scale()
    }

    fn message_plane_display(&mut self, id: u32) -> Result<Option<u32>> {
        (**self).message_plane_display(id)
    }

    fn submit_message(
        &mut self,
        surface: &crate::graphics::Surface,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        (**self).submit_message(surface, rectangle, at)
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
    ) -> Result<bool> {
        (**self).update_image_list(list, submit, force)
    }

    fn update_image_list_animations(
        &mut self,
        list: &mut crate::graphics::ImageList,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        (**self).update_image_list_animations(list, now, budget)
    }

    fn restart_image_list_animation(
        &mut self,
        list: &mut crate::graphics::ImageList,
        key: u32,
        budget: usize,
    ) -> Result<bool> {
        (**self).restart_image_list_animation(list, key, budget)
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        (**self).create_task_child(kind, name, context)
    }

    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        (**self).canvas_layout()
    }

    fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
        (**self).resource_services()
    }

    fn input_device(&mut self) -> Option<&mut dyn crate::input::InputDevice> {
        (**self).input_device()
    }
    fn input_coordinates(&self) -> crate::input::CoordinateMap {
        (**self).input_coordinates()
    }
    fn input_pointer_control(&self) -> bool {
        (**self).input_pointer_control()
    }

    fn surface_size(&self) -> [u32; 2] {
        (**self).surface_size()
    }

    fn next_task_child(&self, previous: Option<u32>) -> Result<Option<u32>> {
        (**self).next_task_child(previous)
    }

    fn parent_task_id(&self) -> Result<u32> {
        (**self).parent_task_id()
    }

    fn dispatch_task_command(
        &mut self,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        (**self).dispatch_task_command(command, buffers, output)
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> TaskEffects {
        (**self).take_task_effects(queued_messages)
    }

    fn dispatch_input_command(
        &mut self,
        state: &mut crate::input::CommandState,
        delivery: crate::input::InputDelivery,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        (**self).dispatch_input_command(state, delivery, command, buffers, output)
    }

    fn dispatch_graphics_event(
        &mut self,
        response: &mut u32,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        (**self).dispatch_graphics_event(response, command, buffers, output)
    }

    fn dispatch_button_event(
        &mut self,
        button: &mut crate::input::ButtonObject,
        command: crate::task::Command,
        buffers: &mut crate::task::CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        (**self).dispatch_button_event(button, command, buffers, output)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub native_index: u32,
    pub status: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugState {
    pub line: u32,
    pub source: Vec<u8>,
    pub scopes: Vec<u32>,
    pub variables: Vec<u32>,
    pub step_over_return: Option<u32>,
}

pub struct Interpreter {
    program: Program,
    boundaries: BTreeMap<usize, usize>,
    pub pc: usize,
    memory: Vec<u8>,
    allocations: BTreeMap<usize, Vec<u8>>,
    external: SharedMemory,
    sp: usize,
    frame_top: usize,
    frame_base: usize,
    stack_start: usize,
    frame_floors: Vec<usize>,
    switch: u32,
    debug: DebugState,
    debug_variable_floors: Vec<usize>,
    messages: messages::MessageState,
    execution_mode: u32,
    native_arguments: Option<usize>,
}

impl Interpreter {
    pub fn new(program: Program, external: Vec<u8>) -> Result<Self> {
        if external.len() > read::MAX_BYTES {
            return Err(Error::new(0, "外部存储超过上限"));
        }
        let stack_start = program.data.len();
        let mut memory = program.data.clone();
        memory.resize(stack_start + program.stack_size, 0);
        let boundaries = program
            .instructions
            .iter()
            .enumerate()
            .map(|(i, ins)| (ins.offset, i))
            .collect();
        Ok(Self {
            program,
            boundaries,
            pc: 0,
            memory,
            allocations: BTreeMap::new(),
            external: SharedMemory::new(external)?,
            sp: stack_start,
            frame_top: stack_start,
            frame_base: stack_start,
            stack_start,
            frame_floors: vec![stack_start],
            switch: 0,
            debug: DebugState::default(),
            debug_variable_floors: Vec::new(),
            messages: messages::MessageState::default(),
            execution_mode: 1,
            native_arguments: None,
        })
    }
    /// 当前解释器执行的程序。
    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn debug(&self) -> &DebugState {
        &self.debug
    }
    pub fn execution_mode(&self) -> u32 {
        self.execution_mode
    }
    pub fn set_execution_mode(&mut self, mode: u32) {
        self.execution_mode = mode;
        self.debug.step_over_return = None;
    }
    /// 初始栈底之后的已用字节；SP 进入初始化数据区时返回空切片。
    pub fn stack_bytes(&self) -> &[u8] {
        &self.memory[self.stack_start.min(self.sp)..self.sp]
    }
    /// 替换为独立外部区并返回旧内容；其他绑定继续持有旧存储。
    pub fn replace_external(&mut self, external: Vec<u8>) -> Result<Vec<u8>> {
        if external.len() > read::MAX_BYTES {
            return Err(Error::new(self.pc, "外部存储超过上限"));
        }
        let external = SharedMemory::new(external)?;
        self.bind_external(external).into_vec()
    }

    pub fn external_storage(&self) -> SharedMemory {
        self.external.clone()
    }

    pub fn bind_external(&mut self, external: SharedMemory) -> SharedMemory {
        std::mem::replace(&mut self.external, external)
    }
    /// 从入口重新执行，保留程序数据、外部区及调试元数据。
    pub fn reset_execution(&mut self) {
        self.pc = 0;
        self.sp = self.stack_start;
        self.frame_base = self.stack_start;
        self.frame_top = self.stack_start;
        self.frame_floors.clear();
        self.frame_floors.push(self.stack_start);
        self.debug_variable_floors.clear();
    }
    pub fn read(&self, address: u32, length: usize) -> Result<MemoryRead<'_>> {
        let offset = (address & 0x7fff_ffff) as usize;
        if address & 0x8000_0000 != 0 {
            Ok(MemoryRead::Shared(self.external.read(offset, length)?))
        } else {
            let (storage, offset) = self.local_region(offset)?;
            Ok(MemoryRead::Local(read::range(storage, offset, length)?))
        }
    }

    fn local_region(&self, offset: usize) -> Result<(&[u8], usize)> {
        if offset < self.memory.len() || self.allocations.is_empty() {
            return Ok((&self.memory, offset));
        }
        let (base, bytes) = self
            .allocations
            .range(..=offset)
            .next_back()
            .ok_or_else(|| Error::new(self.pc, "KCS 分配地址无效"))?;
        Ok((bytes, offset - base))
    }

    pub(crate) fn allocate_bytes(&mut self, bytes: Vec<u8>) -> Result<u32> {
        let base = self
            .allocations
            .last_key_value()
            .map_or(self.memory.len(), |(base, bytes)| base + bytes.len());
        if bytes.is_empty()
            || base
                .checked_add(bytes.len())
                .is_none_or(|end| end > read::MAX_BYTES)
        {
            return Err(Error::new(self.pc, "KCS 分配大小无效"));
        }
        self.allocations.insert(base, bytes);
        Ok(base as u32)
    }

    pub fn read_string(&self, address: u32) -> Result<MemoryRead<'_>> {
        let offset = (address & 0x7fff_ffff) as usize;
        if address & 0x8000_0000 != 0 {
            let bytes = self.external.read(0, self.external.len())?;
            let length = read::zstr(&bytes, offset)?.len();
            Ok(MemoryRead::Shared(std::cell::Ref::map(bytes, |bytes| {
                &bytes[offset..offset + length]
            })))
        } else {
            let (storage, offset) = self.local_region(offset)?;
            Ok(MemoryRead::Local(read::zstr(storage, offset)?))
        }
    }
    pub fn write(&mut self, address: u32, bytes: &[u8]) -> Result<()> {
        let mut p = (address & 0x7fff_ffff) as usize;
        if address & 0x8000_0000 != 0 {
            if p.checked_add(bytes.len())
                .is_none_or(|end| end > self.external.len())
            {
                return Err(Error::new(self.pc, "KCS 写入越界"));
            }
            return self.external.write(p, bytes);
        }
        let storage = if p < self.memory.len() || self.allocations.is_empty() {
            &mut self.memory
        } else {
            let (base, storage) = self
                .allocations
                .range_mut(..=p)
                .next_back()
                .ok_or_else(|| Error::new(self.pc, "KCS 分配地址无效"))?;
            p -= *base;
            storage
        };
        let end = p
            .checked_add(bytes.len())
            .filter(|&n| n <= storage.len())
            .ok_or_else(|| Error::new(self.pc, "KCS 写入越界"))?;
        storage[p..end].copy_from_slice(bytes);
        Ok(())
    }
    fn push(&mut self, n: u32) -> Result<()> {
        let end = self
            .sp
            .checked_add(4)
            .filter(|&n| n <= self.memory.len())
            .ok_or_else(|| Error::new(self.pc, "KCS 栈溢出"))?;
        self.memory[self.sp..end].copy_from_slice(&n.to_le_bytes());
        self.sp = end;
        Ok(())
    }
    fn pop(&mut self) -> Result<u32> {
        // 保存的帧顶同样可被栈来源操作数消费，弹栈只受整块存储的下界限制。
        if self.sp < 4 {
            return Err(Error::new(self.pc, "KCS 操作栈下溢"));
        }
        self.sp -= 4;
        read::u32_at(&self.memory, self.sp)
    }
    fn raw(&mut self, op: Operand) -> Result<u32> {
        if op.flags & 0x40 != 0 {
            self.pop()
        } else {
            Ok(op.payload)
        }
    }
    fn value(&mut self, op: Operand) -> Result<u32> {
        let raw = self.raw(op)?;
        if op.flags & 0x10 == 0 {
            return Ok(raw);
        }
        let width = width(op.flags)?;
        let bytes = self.read(raw, width)?;
        Ok(match width {
            1 if op.flags & 0x20 != 0 => bytes[0] as i8 as i32 as u32,
            1 => bytes[0] as u32,
            2 if op.flags & 0x20 != 0 => {
                i16::from_le_bytes(bytes.try_into().unwrap()) as i32 as u32
            }
            2 => u16::from_le_bytes(bytes.try_into().unwrap()) as u32,
            _ => u32::from_le_bytes(bytes.try_into().unwrap()),
        })
    }
    fn jump(&mut self, target: u32) -> Result<()> {
        if target as usize != self.program.code_len
            && !self.boundaries.contains_key(&(target as usize))
        {
            return Err(Error::new(self.pc, "KCS 目标不在指令边界"));
        }
        self.pc = target as usize;
        Ok(())
    }
    pub fn run(&mut self, host: &mut impl Host, budget: usize) -> Result<Step<Event>> {
        for _ in 0..budget {
            if self.pc == self.program.code_len {
                return Ok(Step::Halted);
            }
            let i = *self
                .boundaries
                .get(&self.pc)
                .ok_or_else(|| Error::new(self.pc, "PC 不在指令边界"))?;
            let ins = self.program.instructions[i].clone();
            if ins.opcode == 0x3b {
                return Ok(Step::Halted);
            }
            if self.execution_mode == 3 && ins.opcode == 0x2a {
                self.execution_mode = 1;
                self.debug.step_over_return = Some(self.pc as u32 + 11);
            }
            self.pc += 1 + 5 * ins.operands.len();
            let a = &ins.operands;
            match ins.opcode {
                0x00 => (),
                0x01..=0x07 | 0x09..=0x0b | 0x0d..=0x14 => {
                    let left = self.value(a[0])?;
                    let right = self.value(a[1])?;
                    let n = numeric::binary(
                        ins.opcode, left, right, a[0].flags, a[1].flags, ins.offset,
                    )?;
                    self.push(n)?;
                }
                0x08 | 0x0c | 0x1a => {
                    let n = self.value(a[0])?;
                    self.push(match ins.opcode {
                        8 => !n,
                        12 => (n == 0) as u32,
                        _ if a[0].flags & 4 != 0 => n ^ 0x8000_0000,
                        _ => n.wrapping_neg(),
                    })?;
                }
                0x15 => {
                    let target = self.raw(a[0])?;
                    let n = self.value(a[1])?;
                    let width = width(a[0].flags)?;
                    self.write(target, &n.to_le_bytes()[..width])?;
                    self.push(match width {
                        1 => n & 255,
                        2 => n & 65535,
                        _ => n,
                    })?;
                }
                0x16..=0x19 => {
                    let target = self.raw(a[0])?;
                    let old = read::u32_at(&self.read(target, 4)?, 0)?;
                    let width = width(a[0].flags & 0x7f)?;
                    let subtract = matches!(ins.opcode, 0x17 | 0x19);
                    let result = if a[0].flags & 15 == 4 {
                        numeric::float(if subtract { 2 } else { 1 }, old, 1f32.to_bits())
                    } else {
                        old.wrapping_add(if subtract { u32::MAX } else { 1 })
                    };
                    self.write(target, &result.to_le_bytes()[..width])?;
                    let result = if ins.opcode >= 0x18 {
                        old
                    } else {
                        read::u32_at(&self.read(target, 4)?, 0)?
                    };
                    self.push(result)?;
                }
                0x1b..=0x24 => {
                    let target = self.raw(a[0])?;
                    let right = self.value(a[1])?;
                    let left = read::u32_at(&self.read(target, 4)?, 0)?;
                    let n = numeric::binary(
                        ins.opcode, left, right, a[0].flags, a[1].flags, ins.offset,
                    )?;
                    self.write(target, &n.to_le_bytes()[..width(a[0].flags)?])?;
                    self.push(n)?;
                }
                0x25 => {
                    if a[0].flags & 0x10 != 0 {
                        let address = self.raw(a[0])?;
                        let n = read::u32_at(&self.read(address, 4)?, 0)?;
                        self.push(n)?;
                    } else if a[0].flags & 0x40 == 0 {
                        self.push(a[0].payload)?;
                    }
                }
                0x26 => {
                    let target = self.raw(a[0])?;
                    let n = self.pop()?;
                    self.write(target, &n.to_le_bytes()[..width(a[0].flags & 0x7f)?])?;
                }
                0x27..=0x29 => {
                    let target = self.value(a[0])?;
                    let jump = if ins.opcode == 0x29 {
                        true
                    } else {
                        let v = self.pop()?;
                        if ins.opcode == 0x27 { v == 0 } else { v != 0 }
                    };
                    self.sp = self.frame_top;
                    if jump {
                        self.jump(target)?;
                    }
                }
                0x2a => {
                    let target = self.value(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 调用参数下溢"));
                    }
                    let base = self.sp - bytes;
                    self.push(self.pc as u32)?;
                    self.push(self.frame_base as u32)?;
                    self.push(self.frame_top as u32)?;
                    self.frame_base = base;
                    self.frame_top = self.sp;
                    self.jump(target)?;
                    self.frame_floors.push(self.frame_top);
                    self.debug_variable_floors.push(self.debug.variables.len());
                }
                0x2b => {
                    let index = self.value(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 宿主参数下溢"));
                    }
                    let old_sp = self.sp;
                    self.sp -= bytes;
                    let arguments = self.memory[self.sp..old_sp].to_vec();
                    let previous = self.native_arguments.replace(self.sp);
                    let result = host.call(index, &arguments, self);
                    self.native_arguments = previous;
                    let result = result?;
                    let status = match result {
                        NativeReturn::Yield => 0,
                        NativeReturn::Value(_) => 1,
                        NativeReturn::Continue => 2,
                        NativeReturn::YieldValue(_) => 3,
                        NativeReturn::YieldVoid => 4,
                        NativeReturn::Retry => 5,
                    };
                    if let NativeReturn::Value(n) | NativeReturn::YieldValue(n) = result {
                        self.push(n)?;
                    }
                    if result == NativeReturn::Retry {
                        self.sp = old_sp;
                        self.pc = ins.offset;
                    }
                    if !matches!(result, NativeReturn::Value(_) | NativeReturn::Continue) {
                        return Ok(Step::Event(Event {
                            native_index: index,
                            status,
                        }));
                    }
                }
                0x2c => {
                    if self.frame_floors.len() < 2 {
                        return Err(Error::new(ins.offset, "KCS return 没有调用帧"));
                    }
                    let result_len = self.value(a[0])? as usize;
                    let parameter_len = self.value(a[1])? as usize;
                    if result_len > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 返回值下溢"));
                    }
                    let result = self.memory[self.sp - result_len..self.sp].to_vec();
                    let saved = self
                        .frame_base
                        .checked_add(parameter_len)
                        .filter(|&n| n + 12 == *self.frame_floors.last().unwrap())
                        .ok_or_else(|| Error::new(ins.offset, "KCS 返回帧无效"))?;
                    let pc = read::u32_at(&self.memory, saved)?;
                    let base = read::u32_at(&self.memory, saved + 4)? as usize;
                    let top = read::u32_at(&self.memory, saved + 8)? as usize;
                    let destination = self.frame_base;
                    // 保存的帧顶可以进入数据区，且不要求位于恢复的基址之后。
                    if base > self.memory.len()
                        || top > self.memory.len()
                        || destination + result_len > self.memory.len()
                    {
                        return Err(Error::new(ins.offset, "KCS 保存帧越界"));
                    }
                    self.frame_base = base;
                    self.frame_top = top;
                    self.sp = destination + result_len;
                    self.frame_floors.pop();
                    self.debug.scopes.pop();
                    if let Some(floor) = self.debug_variable_floors.pop() {
                        self.debug.variables.truncate(floor);
                    }
                    self.memory[destination..self.sp].copy_from_slice(&result);
                    self.jump(pc)?;
                }
                0x2d => {
                    self.pop()?;
                }
                0x2e => {
                    let n = self.raw(a[0])?;
                    self.push(n)?;
                }
                0x2f => {
                    let address = self.raw(a[0])?;
                    let n = u32::from_le_bytes(self.read(address, 4)?.try_into().unwrap());
                    self.push(n)?;
                }
                0x30 => {
                    let target = self.raw(a[0])?;
                    let bytes = self.value(a[1])? as usize;
                    if bytes > self.sp - self.frame_top {
                        return Err(Error::new(ins.offset, "KCS 块复制源下溢"));
                    }
                    let source = self.memory[self.sp - bytes..self.sp].to_vec();
                    self.write(target, &source)?;
                }
                0x31 | 0x32 => {
                    let bytes = self.value(a[0])? as usize;
                    let top = if ins.opcode == 0x31 {
                        self.frame_top.checked_add(bytes)
                    } else {
                        self.frame_top.checked_sub(bytes)
                    }
                    .filter(|&n| n >= self.stack_start && n <= self.memory.len())
                    .ok_or_else(|| Error::new(ins.offset, "KCS 局部帧越界"))?;
                    if top < *self.frame_floors.last().unwrap() {
                        return Err(Error::new(ins.offset, "释放超过当前帧"));
                    }
                    self.frame_top = top;
                    self.sp = top;
                }
                0x33 => self.switch = self.pop()?,
                0x34 => {
                    let value = self.value(a[0])?;
                    let target = self.value(a[1])?;
                    if value == self.switch {
                        self.jump(target)?;
                    }
                }
                0x35 => {
                    let left = self.value(a[0])?;
                    let right = self.value(a[1])?;
                    let target = self.value(a[2])?;
                    if (left.min(right)..=left.max(right)).contains(&self.switch) {
                        self.jump(target)?;
                    }
                }
                0x36..=0x39 => {
                    let n = self.value(a[0])?;
                    self.push(numeric::convert(ins.opcode, n))?;
                }
                0x3a => {
                    let n = self.pop()?;
                    self.push(n)?;
                    self.push(n)?;
                }
                0x3c => self.sp = self.frame_top,
                0x3d => {
                    let n = self.value(a[0])?;
                    self.push(n.wrapping_add(self.frame_base as u32))?;
                }
                0x3e => {
                    let mut n = self.raw(a[0])?;
                    if a[0].flags & 0x10 != 0 {
                        n = u32::from_le_bytes(self.read(n, 4)?.try_into().unwrap());
                    }
                    let displacement = self.value(a[1])?;
                    self.push(n.wrapping_add(displacement))?;
                }
                0x41 => {
                    let line = self.value(a[0])?;
                    let source = self.value(a[1])?;
                    let source = self.read_string(source)?.to_vec();
                    self.debug.source = source;
                    self.debug.line = line;
                }
                0x42 => {
                    let address = self.value(a[0])?;
                    self.read(address, 0)?;
                    if self.debug.scopes.len() >= read::MAX_ITEMS {
                        return Err(Error::new(ins.offset, "KCS 调试作用域超过上限"));
                    }
                    self.debug.scopes.push(address);
                }
                0x43 => {
                    let first = self.value(a[0])?;
                    let count = self.value(a[1])?;
                    if count as usize > read::MAX_ITEMS - self.debug.variables.len() {
                        return Err(Error::new(ins.offset, "KCS 调试变量超过上限"));
                    }
                    self.debug
                        .variables
                        .extend((0..count).map(|i| first.wrapping_add(i)));
                }
                0x44 => {
                    let count = self.value(a[0])? as usize;
                    let len = self
                        .debug
                        .variables
                        .len()
                        .checked_sub(count)
                        .ok_or_else(|| Error::new(ins.offset, "KCS 调试变量下溢"))?;
                    self.debug.variables.truncate(len);
                }
                _ => {
                    return Err(Error::new(
                        ins.offset,
                        format!("KCS 指令 {:02x} 的执行契约尚未实现", ins.opcode),
                    ));
                }
            }
        }
        Ok(Step::Yield)
    }
}

fn width(flags: u8) -> Result<usize> {
    if flags & 0x80 != 0 {
        return Ok(4);
    }
    match flags & 15 {
        1 => Ok(1),
        2 => Ok(2),
        3 | 4 => Ok(4),
        _ => Err(Error::new(0, "未支持的 KCS 存储类型")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Interpreter, Program, strings};

    #[test]
    fn allocated_regions_preserve_stack_limits_and_independent_boundaries() {
        let mut vm =
            Interpreter::new(Program::from_code(&[], vec![0; 8], 4).unwrap(), vec![]).unwrap();
        let first = vm.allocate_bytes(b"a b\0".to_vec()).unwrap();
        let second = vm.allocate_bytes(b"tail\0".to_vec()).unwrap();
        assert_eq!((first, second), (12, 16));
        assert!(vm.read(11, 2).is_err());
        assert!(vm.read(first + 3, 2).is_err());
        assert!(vm.write(first + 3, b"xy").is_err());
        assert_eq!(strings::word_offset(&vm, first, 1).unwrap(), 2);
        vm.write(first + 2, b"c").unwrap();
        vm.push(1).unwrap();
        assert!(vm.push(2).is_err());
        vm.reset_execution();
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(vm.read_string(first).unwrap(), b"a c");
        assert_eq!(vm.read_string(second).unwrap(), b"tail");
    }
}
