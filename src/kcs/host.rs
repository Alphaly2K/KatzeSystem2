use super::{Host, Interpreter, MemoryCopyMode, NativeReturn, ScriptRandom, TaskClock};
use super::{memory, numeric, strings};
use crate::task::TaskTree;
use crate::{Error, HandleAllocator, Result, expression, read, text};
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

/// 普通任务表的持久宿主状态；数值区由 `CoreHost::state` 单独提供，
/// 以便同一份状态在多次 native 调用之间保留集合句柄、随机序列和请求队列。
#[derive(Default)]
pub struct HostState {
    pub configured_shared_slots: u32,
    pub debug_messages: Vec<Vec<u8>>,
    pub menu_item_requests: Vec<MenuItemRequest>,
    pub task_time: u32,
    pub task_origin: u32,
    pub task_id: u32,
    pub system_time_ms: u32,
    pub debug_mode: u32,
    /// 新声音分类配置的启用字段；与声音管理器或设备是否存在分别保存。
    pub sound_config_enabled: u32,
    pub sound_config: Option<crate::audio::SoundConfig>,
    pub sound_groups: Option<crate::audio::SoundGroups>,
    pub sound_settings_dirty: u32,
    /// SOUND/stereo 覆盖值；缺省使用配置构造值一。
    pub sound_stereo: Option<u32>,
    pub sequence_name_mode: u32,
    pub surface_size: [u32; 2],
    /// 消息记录生成显示时间时使用的全局比例；None 表示尚无时间上下文。
    pub message_time_scale: Option<u32>,
    pub message_defaults: Option<crate::message::MessageDefaults>,
    pub system_settings: Option<crate::settings::SystemSettings>,
    pub languages: crate::language::LanguageState,
    pub save_services: Option<Box<dyn crate::save::SaveServices>>,
    /// 外层输入宿主提供的当前逻辑画布坐标；保留画布外的值。
    pub pointer_position: [i32; 2],
    pub memory_copy_mode: MemoryCopyMode,
    pub random: ScriptRandom,
    integer_maps: BTreeMap<u32, BTreeMap<Vec<u8>, u32>>,
    sequence_maps: BTreeMap<u32, super::sequences::SequenceMap>,
    pub(super) containers: BTreeMap<u32, crate::save::DataContainer>,
    handles: HandleAllocator,
}

impl HostState {
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
        self.host.sound_stereo.unwrap_or(1)
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
        call_native(
            self.state,
            &mut self.host,
            self.task_tree,
            index,
            arguments,
            vm,
        )
    }
}

/// 普通任务表的 native 分派；宿主状态由调用者长期持有。
pub(crate) fn call_native(
    state: &mut expression::State,
    host: &mut HostState,
    task_tree: Option<&TaskTree>,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if index == 1062 {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mode = u32::from_le_bytes(arguments[..4].try_into().unwrap());
        let address = u32::from_le_bytes(arguments[4..].try_into().unwrap());
        if mode <= 1 {
            let bytes = host.languages.expand(vm.read_string(address)?, mode)?;
            write_string(vm, address, &bytes, 1)?;
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 238 | 244 | 246..=248 | 254 | 389 | 469) {
        return super::data::call(host, index, arguments, vm);
    }
    if matches!(index, 359 | 360) {
        let count = if index == 359 { 4 } else { 3 };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        for (index, address) in arguments.chunks_exact(4).enumerate() {
            let address = u32::from_le_bytes(address.try_into().unwrap());
            // 原地址解析器对区外起点返回空指针；地址零仍指向普通区首字节。
            if vm.read(address, 1).is_err() {
                continue;
            }
            let block = host
                .system_settings
                .as_mut()
                .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?
                .block_mut(index);
            if count == 4 {
                vm.write(address, block)?;
            } else {
                block.copy_from_slice(vm.read(address, block.len())?);
            }
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 0 | 2 | 3) {
        return Ok(NativeReturn::Continue);
    }
    if index == 4 {
        return Ok(NativeReturn::Value(host.system_time_ms));
    }
    if index == 68 {
        return Ok(NativeReturn::Value(host.task_id));
    }
    if index == 465 {
        return Ok(NativeReturn::Value(host.debug_mode));
    }
    if index == 938 {
        return Ok(NativeReturn::Value(host.sound_config_enabled));
    }
    if index == 65 {
        return Ok(NativeReturn::YieldVoid);
    }
    if index == 33 {
        return Ok(NativeReturn::Value(host.random.next_integer()));
    }
    if index == 34 {
        return Ok(NativeReturn::Value(host.random.next_float().to_bits()));
    }
    if matches!(index, 419 | 515) {
        if host.integer_maps.len() + host.sequence_maps.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "KCS 集合对象数量超过上限"));
        }
        let handle = host.handles.allocate_at(vm.pc)?;
        if index == 419 {
            host.sequence_maps.insert(handle, Default::default());
        } else {
            host.integer_maps.insert(handle, BTreeMap::new());
        }
        return Ok(NativeReturn::Value(handle));
    }
    let count = match index {
        1 | 6 | 17 | 18 | 25 | 26 | 40 | 47 | 48 | 71 | 209 | 210 | 212 | 213 | 278 | 319 | 75
        | 393 | 420 | 421 | 516 => 1,
        8 | 13..=16 | 19..=24 | 149 | 211 | 276 | 277 | 422 | 423 | 518 => 2,
        50..=58 | 60 | 62..=64 => 1,
        59 | 61 => 2,
        38 | 44 | 45 | 790 => 4,
        35..=37 | 41 | 42 | 517 | 519 => 3,
        5 | 76 | 789 => 0,
        _ => return Err(Error::new(vm.pc, format!("未支持的 KCS native {index}"))),
    };
    if arguments.len() != count * 4 {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    let args: Vec<_> = arguments
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    Ok(match index {
        1 => {
            host.debug_messages.push(strings::format(vm, args[0], 0)?);
            NativeReturn::Continue
        }
        6 => NativeReturn::Value(args[0]),
        393 => {
            if host.menu_item_requests.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "KCS 菜单请求数量超过上限"));
            }
            host.menu_item_requests.push(MenuItemRequest {
                task_id: host.task_id,
                command_id: 0x77,
                value: args[0],
            });
            NativeReturn::Continue
        }
        420 => {
            if args[0] != 0 && host.sequence_maps.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS 序列对象句柄无效"));
            }
            NativeReturn::Continue
        }
        421..=423 => {
            if args[0] == 0 {
                return Ok(if index == 423 {
                    NativeReturn::Value(0)
                } else {
                    NativeReturn::Continue
                });
            }
            let map = host
                .sequence_maps
                .get_mut(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "KCS 序列对象句柄无效"))?;
            if index == 421 {
                map.clear();
                NativeReturn::Continue
            } else {
                let source = strings::format(vm, args[1], 0)?;
                if index == 422 {
                    map.register(&source, host.sequence_name_mode)?;
                    NativeReturn::Continue
                } else {
                    NativeReturn::Value(u32::from(map.contains(&source)?))
                }
            }
        }
        8 | 149 => {
            let values = if index == 8 {
                host.surface_size
            } else {
                host.pointer_position.map(|coordinate| coordinate as u32)
            };
            for (address, value) in args.iter().zip(values) {
                vm.write(*address, &value.to_le_bytes())?;
            }
            NativeReturn::Continue
        }
        5 => NativeReturn::Value(host.task_time.wrapping_sub(host.task_origin)),
        71 => {
            vm.wait_for_message(args[0])?;
            NativeReturn::YieldVoid
        }
        75 | 76 => {
            let tree = task_tree.ok_or_else(|| Error::new(vm.pc, "KCS 宿主未提供任务树"))?;
            hierarchy_query(
                tree,
                host.task_id,
                index,
                args.first().copied().unwrap_or(0),
                vm.pc,
            )?
        }
        35 => {
            memory::copy(
                vm,
                args[0],
                args[1],
                read::sized(args[2], vm.pc)?,
                host.memory_copy_mode,
            )?;
            NativeReturn::Continue
        }
        36 => {
            let count = read::sized(args[2], vm.pc)?;
            vm.read(args[0], count)?;
            vm.write(args[0], &vec![args[1] as u8; count])?;
            NativeReturn::Continue
        }
        37 => {
            let value = args[1] as i32;
            let mut digits = Vec::new();
            // MIN 的绝对值仍为负数，负余数直接参与数字字节计算。
            let mut n = value.wrapping_abs();
            if n == 0 {
                digits.push(b'0');
            } else {
                let mut group = 0;
                while n != 0 {
                    if group == 3 {
                        digits.push(b',');
                        group = 0;
                    }
                    digits.push(((n % 10) as u8).wrapping_add(b'0'));
                    n /= 10;
                    group += 1;
                }
                if value < 0 {
                    digits.push(b'-');
                } else if args[2] != 0 {
                    digits.push(b'+');
                }
            }
            digits.reverse();
            write_string(vm, args[0], &digits, 2)?;
            NativeReturn::Continue
        }
        319 => {
            let shifted = numeric::float_ieee(1, args[0], 0.5f32.to_bits());
            let rounded = numeric::native_math(15, &[shifted]);
            NativeReturn::Value(numeric::convert(0x39, rounded))
        }
        50..=61 => {
            let function = match index {
                50..=55 => index - 44,
                56..=61 => index - 43,
                _ => unreachable!(),
            };
            let mut value = numeric::native_math(function, &args);
            // 普通表调用 SSE2 precise pow，奇整数指数会清除 NaN 底数符号。
            let exponent = args.get(1).copied().map(f32::from_bits).unwrap_or(0.0);
            if index == 61
                && f32::from_bits(args[0]).is_nan()
                && exponent.abs() < 16_777_216.0
                && exponent.fract() == 0.0
                && exponent as i32 & 1 != 0
            {
                value &= 0x7fff_ffff;
            }
            NativeReturn::Value(value)
        }
        62 | 63 => {
            let (multiplier, divisor) = if index == 62 {
                (std::f32::consts::PI.to_bits(), 180.0f32.to_bits())
            } else {
                (180.0f32.to_bits(), std::f32::consts::PI.to_bits())
            };
            let product = numeric::float_ieee(3, args[0], multiplier);
            NativeReturn::Value(numeric::float_ieee(4, product, divisor))
        }
        64 => NativeReturn::Value((args[0] as i32).wrapping_abs() as u32),
        13 => {
            let source = vm.read_string(args[1])?.to_vec();
            if args[0] > args[1] && args[0] - args[1] <= source.len() as u32 {
                return Err(Error::new(vm.pc, "KCS 字符串复制会覆盖尚未读取的源"));
            }
            write_string(vm, args[0], &source, 2)?;
            NativeReturn::Value(args[0])
        }
        14 => {
            let text = strings::format(vm, args[1], 0)?;
            write_string(vm, args[0], &text, 2)?;
            NativeReturn::Value(args[0])
        }
        15 => {
            strings::append_raw(vm, args[0], args[1])?;
            NativeReturn::Value(args[0])
        }
        16 => {
            let source = strings::format(vm, args[1], 0)?;
            let length = vm.read_string(args[0])?.len();
            write_string(vm, args[0].wrapping_add(length as u32), &source, 2)?;
            NativeReturn::Value(args[0])
        }
        17 => NativeReturn::Value(vm.read_string(args[0])?.len() as u32),
        18 => NativeReturn::Value(strings::format(vm, args[0], 0)?.len() as u32),
        19..=22 => {
            let (left, right) = if index.is_multiple_of(2) {
                (
                    strings::format(vm, args[0], 0)?,
                    strings::format(vm, args[1], 0)?,
                )
            } else {
                (
                    vm.read_string(args[1])?.to_vec(),
                    vm.read_string(args[0])?.to_vec(),
                )
            };
            NativeReturn::Value(strings::compare(&left, &right, index >= 21) as u32)
        }
        26 => NativeReturn::Value(strings::float32(&strings::format(vm, args[0], 0)?)),
        25 => NativeReturn::Value(
            text::integer_prefix(&strings::format(vm, args[0], 0)?).map_or(0, |(_, value)| value),
        ),
        23 => {
            let source = vm.read_string(args[0])?;
            let mut length = 0;
            while length < source.len() {
                let width = strings::character_width(&source[length..]);
                if length + width >= args[1] as usize || length + width > source.len() {
                    break;
                }
                length += width;
            }
            vm.write(args[0].wrapping_add(length as u32), &[0, 0])?;
            NativeReturn::Value(length as u32)
        }
        24 => {
            let source = vm.read_string(args[0])?;
            let mut length = 0;
            for _ in 0..args[1] {
                if length == source.len() {
                    break;
                }
                length += strings::character_width(&source[length..]);
            }
            vm.write(args[0].wrapping_add(length as u32), &[0, 0])?;
            NativeReturn::Value(length as u32)
        }
        40 => NativeReturn::Value((strings::character_width(vm.read_string(args[0])?) == 2) as u32),
        47 | 48 => {
            let source = if index == 48 {
                strings::format(vm, args[0], 0)?
            } else {
                vm.read_string(args[0])?.to_vec()
            };
            let mut length = 0;
            let mut count = 0;
            while length < source.len() {
                length += strings::character_width(&source[length..]);
                count += 1;
            }
            NativeReturn::Value(count)
        }
        38 => {
            let separators = vm.read_string(args[2])?;
            let mut decoded = Vec::new();
            let mut p = 0;
            while p < separators.len() {
                if separators[p..].starts_with(b"\\t") {
                    decoded.push(b'\t');
                    p += 2;
                } else {
                    decoded.push(separators[p]);
                    p += 1;
                }
            }
            let source = vm.read_string(args[1])?;
            let length = source
                .iter()
                .position(|b| decoded.contains(b))
                .unwrap_or(source.len())
                .min(args[3].wrapping_sub(2) as usize);
            let value = source[..length].to_vec();
            write_string(vm, args[0], &value, 2)?;
            NativeReturn::Value(length as u32)
        }
        41 => {
            let value = strings::split(vm.read_string(args[1])?, vm.read_string(args[2])?);
            vm.write(args[0], &value)?;
            NativeReturn::Continue
        }
        42 => {
            let value = strings::multi(vm, args[1], args[2] as i32)?.to_vec();
            write_string(vm, args[0], &value, 2)?;
            NativeReturn::Value((!value.is_empty()) as u32)
        }
        44 => {
            vm.write(args[0], &[0])?;
            let first = (args[2] as i32).max(0);
            let last = if args[3] == u32::MAX {
                i32::MAX
            } else {
                args[3] as i32
            };
            let value = strings::join(vm, args[1], first, last)?;
            write_string(vm, args[0], &value, 1)?;
            NativeReturn::Continue
        }
        // 状态已经由调用者初始化，原入口在已有数值区时同样保留它。
        209 => NativeReturn::Continue,
        210 => NativeReturn::Value(state.shared.get(args[0] as usize).copied().unwrap_or(0) as u32),
        211 => {
            if let Some(slot) = state.shared.get_mut(args[0] as usize) {
                *slot = args[1] as i32;
            }
            NativeReturn::Continue
        }
        212 => {
            state.current = args[0] as i32;
            NativeReturn::Continue
        }
        213 => {
            let source = text::ascii(vm.read_string(args[0])?)?;
            NativeReturn::Value(expression::eval_parameter(source, state)? as u32)
        }
        276 => {
            state
                .strings
                .insert(args[0], vm.read_string(args[1])?.to_vec());
            NativeReturn::Continue
        }
        277 => {
            let string = state.strings.get(&args[0]);
            write_string(vm, args[1], string.map_or(&[], Vec::as_slice), 1)?;
            NativeReturn::Value(string.is_some() as u32)
        }
        278 => {
            let string = text::expand(vm.read_string(args[0])?, &state.strings)?;
            write_string(vm, args[0], &string, 2)?;
            NativeReturn::Continue
        }
        45 => {
            let old = vm.read_string(args[2])?.to_vec();
            let replacement = vm.read_string(args[3])?.to_vec();
            if old.is_empty() {
                return Err(Error::new(vm.pc, "KCS 字符串替换的查找串为空"));
            }
            vm.write(args[0], &[0])?;
            let source = vm.read_string(args[1])?;
            let mut output = Vec::new();
            let mut p = 0;
            while p < source.len() {
                if source[p..].starts_with(&old) {
                    output.extend(&replacement);
                    p += old.len();
                } else {
                    output.push(source[p]);
                    p += 1;
                }
                if output.len() > read::MAX_BYTES {
                    return Err(Error::new(vm.pc, "KCS 字符串替换超过上限"));
                }
            }
            write_string(vm, args[0], &output, 1)?;
            NativeReturn::Continue
        }
        519 => {
            vm.write(args[0], &[0])?;
            let words = text::arguments(vm.read_string(args[1])?)?;
            let word = words.get(args[2] as usize);
            if let Some(word) = word {
                write_string(vm, args[0], word, 2)?;
            }
            NativeReturn::Value(word.is_some() as u32)
        }
        516 => {
            if args[0] != 0 && host.integer_maps.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "无效的 KCS 整数字典句柄"));
            }
            NativeReturn::Continue
        }
        517 | 518 => {
            if args[0] == 0 {
                return Ok(if index == 517 {
                    NativeReturn::Continue
                } else {
                    NativeReturn::Value(u32::MAX)
                });
            }
            let entries = host
                .integer_maps
                .get_mut(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "无效的 KCS 整数字典句柄"))?;
            let key = vm.read_string(args[1])?;
            if index == 518 {
                NativeReturn::Value(entries.get(key).copied().unwrap_or(u32::MAX))
            } else {
                if entries.len() >= read::MAX_ITEMS && !entries.contains_key(key) {
                    return Err(Error::new(vm.pc, "KCS 整数字典条目超过上限"));
                }
                entries.entry(key.to_vec()).or_insert(args[2]);
                NativeReturn::Continue
            }
        }
        789 => NativeReturn::Value(host.configured_shared_slots),
        790 => {
            if args[1] >= 8 {
                return Ok(NativeReturn::Value(0));
            }
            let settings = host
                .system_settings
                .as_ref()
                .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
            let ranges = if args[0] == 0 {
                &settings.local_flag_ranges
            } else {
                &settings.global_flag_ranges
            };
            vm.write(args[2], &ranges[args[1] as usize][0].to_le_bytes())?;
            // 起点输出可以覆盖已退栈的索引参数；两处输出地址已在写入前确定。
            let index = if let Some(base) = vm.native_arguments {
                u32::from_le_bytes(vm.read((base + 4) as u32, 4)?.try_into().unwrap())
            } else {
                args[1]
            };
            let range = ranges
                .get(index as usize)
                .ok_or_else(|| Error::new(vm.pc, "KCS 变量范围索引越界"))?;
            vm.write(args[3], &range[1].to_le_bytes())?;
            NativeReturn::Value(1)
        }
        _ => unreachable!(),
    })
}

fn write_string(
    vm: &mut Interpreter,
    address: u32,
    bytes: &[u8],
    terminators: usize,
) -> Result<()> {
    let mut output = bytes.to_vec();
    output.resize(output.len() + terminators, 0);
    vm.write(address, &output)
}
