//! 用本机游戏目录跑启动脚本，报告首个未接入的宿主入口。
//!
//! 用法：`cargo run --example probe_startup -- <游戏目录>`。

mod support;

use katze_system2::{
    Result,
    input::{InputDevice, KeySample, PointerSample},
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::kind,
};
use std::collections::BTreeMap;
use support::{Game, build_runtime, load_game};

/// 按轮次产生点击的设备；`KATZE_PROBE_CLICKS` 为逗号分隔的 `轮次@x:y` 项。
struct ScriptedDevice {
    polls: u32,
    clicks: Vec<(u32, [i32; 2])>,
}

impl ScriptedDevice {
    fn new() -> Self {
        let clicks = std::env::var("KATZE_PROBE_CLICKS").map_or_else(
            |_| Vec::new(),
            |value| {
                value
                    .split(',')
                    .filter_map(|item| {
                        let (round, position) = item
                            .split_once('@')
                            .map_or((item, "640:360"), |(round, at)| (round, at));
                        let (x, y) = position.split_once(':')?;
                        Some((
                            round.trim().parse().ok()?,
                            [x.trim().parse().ok()?, y.trim().parse().ok()?],
                        ))
                    })
                    .collect()
            },
        );
        Self { polls: 0, clicks }
    }
}

impl InputDevice for ScriptedDevice {
    fn poll(&mut self) -> Result<()> {
        self.polls = self.polls.wrapping_add(1);
        if std::env::var_os("KATZE_TRACE_INPUT").is_some() && self.polls < 320 {
            eprintln!("[input-poll] polls={}", self.polls);
        }
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        // 按下持续两轮，位置再保持两轮，使释放入轮也落在同一区域内。
        let click = self
            .clicks
            .iter()
            .find(|(round, _)| self.polls >= *round && self.polls < round + 4);
        Ok(PointerSample {
            position: click.map_or([640, 360], |(_, position)| *position),
            active: u32::from(click.is_some_and(|(round, _)| self.polls < round + 2)),
        })
    }
    fn key(&mut self, key: u32, group: u32, _: bool) -> Result<KeySample> {
        // 鼠标左键以键零组零掩码位 2 报告。
        let pressed = key == 0
            && group == 0
            && self
                .clicks
                .iter()
                .any(|(round, _)| self.polls >= *round && self.polls < round + 2);
        Ok(KeySample {
            mask: if pressed { 2 } else { 0 },
            repeat: 0,
        })
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn set_pointer(&mut self, _: [i32; 2]) -> Result<()> {
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        Ok(())
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：probe_startup <游戏目录> [帧数]")?;
    let frames: u32 = std::env::args()
        .nth(2)
        .map_or(Ok(600), |value| value.parse())?;
    let mut game = load_game(&root)?;
    if let Some(name) = std::env::var_os("KATZE_PROBE_LIST") {
        let name = name.to_string_lossy().into_owned();
        if let Some(archive) = game.router.archives.get(&name) {
            for (name, raw, offset, length) in archive.entries() {
                println!(
                    "{} raw={:?} {} {}",
                    name,
                    String::from_utf8_lossy(raw),
                    offset,
                    length
                );
            }
        }
    }
    if let Some(name) = std::env::var_os("KATZE_PROBE_PRINT") {
        let name = name.to_string_lossy().into_owned();
        let (resource, _) = name.split_once('@').unwrap_or((&name, ""));
        let data = game
            .router
            .read(resource.as_bytes(), ResourceMode::LooseFirst)?;
        match data {
            Some(data) => {
                println!("打印 {name} 共 {} 字节", data.len());
                println!("{}", String::from_utf8_lossy(&data[..data.len().min(600)]));
                // `name@路径` 形式把完整资源写到指定文件。
                if let Some((_, target)) = name.split_once('@') {
                    std::fs::write(target, &data)?;
                }
            }
            None => println!("打印 {name} 未找到"),
        }
    }

    if let Some(spec) = std::env::var_os("KATZE_PROBE_LIST") {
        let spec = spec.to_string_lossy().into_owned();
        let (prefix, needle) = spec.split_once('|').unwrap_or((&spec, ""));
        let keys: Vec<String> = game.router.archives.keys().cloned().collect();
        for key in keys {
            if !prefix.is_empty() && key != prefix {
                continue;
            }
            let archive = game.router.archives.get_mut(&key).unwrap();
            for (name, _, _, _) in archive.entries() {
                if needle.is_empty() || name.contains(needle) {
                    println!("{key}/{name}");
                }
            }
        }
    }
    if let Some(spec) = std::env::var_os("KATZE_PROBE_FIND") {
        let spec = spec.to_string_lossy().into_owned();
        let (prefix, needle) = spec.split_once('|').unwrap_or(("", spec.as_str()));
        let mut needles = vec![needle.as_bytes().to_vec()];
        needles.push(
            needle
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>(),
        );
        let keys: Vec<String> = game.router.archives.keys().cloned().collect();
        for key in keys {
            if !prefix.is_empty() && key != prefix {
                continue;
            }
            let archive = game.router.archives.get_mut(&key).unwrap();
            let names: Vec<String> = archive
                .entries()
                .map(|(name, _, _, _)| name.to_string())
                .collect();
            for name in names {
                let Some(data) = archive.read(&name)? else {
                    continue;
                };
                for needle in &needles {
                    let mut from = 0;
                    while let Some(position) = data[from..]
                        .windows(needle.len())
                        .position(|window| window == needle)
                    {
                        let at = from + position;
                        let start = at.saturating_sub(80);
                        let end = (at + needle.len() + 160).min(data.len());
                        println!(
                            "{key}/{name} @{at}: {:?}",
                            String::from_utf8_lossy(&data[start..end])
                        );
                        from = at + needle.len();
                    }
                }
            }
        }
    }
    let programs = std::mem::take(&mut game.programs);
    if let Some(spec) = std::env::var_os("KATZE_PROBE_FES") {
        let spec = spec.to_string_lossy().into_owned();
        let data = game
            .router
            .read(spec.as_bytes(), ResourceMode::LooseFirst)?;
        if let Some(data) = data {
            let program = katze_system2::fes::Program::parse(&data)?;
            for declaration in &program.declarations {
                println!("-- {} --", String::from_utf8_lossy(&declaration.header));
                for line in &declaration.lines {
                    println!("{}: {}", line.offset, String::from_utf8_lossy(&line.bytes));
                }
            }
            let interpreter = katze_system2::fes::Interpreter::new(program)?;
            for (name, lines) in interpreter.debug_all_blocks() {
                println!("== {name}");
                for (offset, text) in lines {
                    println!("{offset}: {text}");
                }
            }
        } else {
            println!("FES {spec} 未找到");
        }
    }
    if let Some(spec) = std::env::var_os("KATZE_PROBE_HG3") {
        let spec = spec.to_string_lossy().into_owned();
        if let Some(data) = game
            .router
            .read(spec.as_bytes(), ResourceMode::LooseFirst)?
        {
            let metadata = katze_system2::hg3::Metadata::parse(&data)?;
            println!("HG3 {spec} 共 {} 帧", metadata.frames.len());
            for frame in &metadata.frames {
                let size = frame
                    .info
                    .as_ref()
                    .map(|info| (info.size, info.offset, info.canvas_size));
                println!("  帧 {} 大小 {:?}", frame.id, size);
            }
        } else {
            println!("HG3 {spec} 未找到");
        }
    }
    if let Some(spec) = std::env::var_os("KATZE_PROBE_INS") {
        let spec = spec.to_string_lossy().into_owned();
        let (name, range) = spec.split_once('@').unwrap_or((spec.as_str(), "0"));
        let (from, to) = range
            .split_once('-')
            .map_or((range, range), |(from, to)| (from, to));
        let (from, to): (usize, usize) =
            (from.parse().unwrap_or(0), to.parse().unwrap_or(usize::MAX));
        if let Some(program) = programs.get(&name.to_ascii_lowercase()) {
            for instruction in program
                .instructions()
                .iter()
                .filter(|instruction| instruction.offset >= from && instruction.offset <= to)
            {
                println!(
                    "    {} {:02x} {:?}",
                    instruction.offset,
                    instruction.opcode,
                    instruction
                        .operands
                        .iter()
                        .map(|operand| (operand.flags, operand.payload))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    if std::env::var_os("KATZE_PROBE_SCAN").is_some() {
        let wanted: Vec<u32> = std::env::var("KATZE_PROBE_SCAN").map_or_else(
            |_| vec![400, 935, 936, 9],
            |value| {
                value
                    .split(',')
                    .filter_map(|item| item.trim().parse().ok())
                    .collect()
            },
        );
        for (name, program) in &programs {
            if !name.ends_with(".kcs") {
                continue;
            }
            let instructions = program.instructions();
            for (position, instruction) in instructions.iter().enumerate() {
                if instruction.opcode == 0x2b
                    && instruction
                        .operands
                        .first()
                        .is_some_and(|operand| wanted.contains(&operand.payload))
                {
                    let first = instruction.operands.first().unwrap().payload;
                    let start = position.saturating_sub(6);
                    let context: Vec<String> = instructions[start..position]
                        .iter()
                        .map(|item| {
                            format!(
                                "{}:{:02x}{:?}",
                                item.offset,
                                item.opcode,
                                item.operands
                                    .iter()
                                    .map(|operand| operand.payload)
                                    .collect::<Vec<_>>()
                            )
                        })
                        .collect();
                    println!("{name} -> {first} @ {} {context:?}", instruction.offset);
                }
            }
        }
    }
    let start_file = game
        .start
        .rsplit('/')
        .next()
        .unwrap_or(&game.start)
        .to_ascii_lowercase();
    let start_instructions = programs
        .get(&start_file)
        .or_else(|| {
            let stem = start_file.trim_end_matches(".kcs").to_string();
            programs.get(&stem)
        })
        .map(|program| program.instructions().to_vec())
        .unwrap_or_default();
    println!(
        "启动指令 {} 末偏移 {:?}",
        start_instructions.len(),
        start_instructions
            .last()
            .map(|instruction| instruction.offset)
    );

    let mut runtime = build_runtime(
        Game {
            root: game.root,
            router: game.router,
            startup: game.startup,
            start: game.start,
            programs,
        },
        [1280, 720],
    )?;
    runtime.set_input_device(ScriptedDevice::new())?;
    let input = runtime.create(0, kind::INPUT, b"input", b"")?;
    let sound = runtime.create(0, kind::SOUND, b"sound", b"")?;
    println!("输入 {input} 声音 {sound}");
    let task = runtime.create(0, kind::ORDINARY, b"main", b"")?;
    println!("主任务 {task}");
    let dump = |vm: &katze_system2::kcs::Interpreter| {
        if let Ok(bytes) = vm.read(2200, 300) {
            for (offset, chunk) in bytes.chunks(64).enumerate() {
                println!(
                    "    {:4}: {:?}",
                    2200 + offset * 64,
                    String::from_utf8_lossy(chunk)
                );
            }
        }
        for instruction in start_instructions.iter().filter(|instruction| {
            instruction.offset + 8 >= vm.pc.saturating_sub(400) && instruction.offset <= vm.pc + 140
        }) {
            println!(
                "    {} {:02x} {:?}",
                instruction.offset,
                instruction.opcode,
                instruction
                    .operands
                    .iter()
                    .map(|operand| (operand.flags, operand.payload))
                    .collect::<Vec<_>>()
            );
        }
        let position = start_instructions.partition_point(|item| item.offset < vm.pc);
        println!("    pc 索引 {position}");
        for instruction in &start_instructions
            [position.saturating_sub(2)..(position + 3).min(start_instructions.len())]
        {
            println!(
                "   *{} {:02x} {:?}",
                instruction.offset,
                instruction.opcode,
                instruction
                    .operands
                    .iter()
                    .map(|operand| operand.payload)
                    .collect::<Vec<_>>()
            );
        }
    };
    let mut now = 0;
    for frame in 0..frames {
        now += 16;
        if let Err(error) = runtime.update(now, 16) {
            println!("第 {frame} 帧 pc {}：{}", error.offset, error.message);
            for id in 0..160 {
                if let Some(vm) = runtime.object(id).and_then(|object| object.interpreter()) {
                    println!("  任务 {id} pc {}", vm.pc);
                    if (id == task
                        || std::env::var_os("KATZE_PROBE_DUMP").is_some_and(|v| v == "all"))
                        && std::env::var_os("KATZE_PROBE_DUMP").is_some()
                    {
                        dump(vm);
                    }
                    // `KATZE_PROBE_MEM=地址:长度,...` 转储失败任务各 VM 的普通区。
                    if let Ok(spec) = std::env::var("KATZE_PROBE_MEM") {
                        for item in spec.split(',') {
                            let Some((address, length)) = item.split_once(':') else {
                                continue;
                            };
                            let (Ok(address), Ok(length)) =
                                (address.parse::<u32>(), length.parse::<u32>())
                            else {
                                continue;
                            };
                            if let Ok(bytes) = vm.read(address, length as usize) {
                                println!(
                                    "  任务 {id} @{address}: {:?} {:?}",
                                    String::from_utf8_lossy(&bytes),
                                    &bytes[..bytes.len().min(64)]
                                );
                            }
                        }
                    }
                }
            }
            break;
        }
        let dump_frame = std::env::var("KATZE_PROBE_DUMP").ok().is_some_and(|value| {
            let frames: Vec<u32> = value
                .split(',')
                .filter_map(|item| item.trim().parse().ok())
                .collect();
            if frames.is_empty() {
                frame == 200
            } else {
                frames.contains(&frame)
            }
        });
        if dump_frame {
            for id in 0..160 {
                if let Some(node) = runtime.tree().node(id) {
                    let object = runtime.object(id);
                    let pc = object
                        .and_then(|object| object.interpreter())
                        .map(|vm| vm.pc);
                    let elapsed = object.map(|object| object.clock().elapsed());
                    let paused = object.map(|object| object.clock().paused);
                    println!(
                        "  节点 {id} 类型 {} 父 {:?} pc {:?} 时钟 {:?} 暂停 {:?}",
                        node.kind, node.parent, pc, elapsed, paused
                    );
                    if let Some(addr) = object
                        .and_then(|o| o.interpreter())
                        .and_then(|vm| vm.waiting_message_address())
                    {
                        println!("    任务 {id} 等待消息缓冲 {addr}");
                    }
                    if let Some(scene) = object.and_then(|o| o.fes_scene()) {
                        let (pending, block, pc) = scene.interpreter().pending_pc();
                        println!(
                            "    FES {id} pending {:?} 退出 {:?} 事件停放 {} 块 {} 指令 {}",
                            scene.is_pending(),
                            scene.exit_code(),
                            pending,
                            block,
                            pc
                        );
                        println!(
                            "    FES {id} 期限 {:?} 暂停 {} 画布偏移 {:?}",
                            scene.deadline(),
                            object.and_then(|o| o.paused_fes()).unwrap_or(u32::MAX),
                            scene.objects().canvas_offset()
                        );
                        if let Some(fes_names) = std::env::var_os("KATZE_PROBE_FESNAMED") {
                            let names = &scene.interpreter().named;
                            for (name, slots) in names {
                                let name = std::str::from_utf8(name.as_bytes()).unwrap_or("");
                                if fes_names.to_string_lossy().split(',').any(|f| name == f) {
                                    println!("    FES {id} 命名 {name} = {slots:?}");
                                }
                            }
                        }
                        if let Some(spec) = std::env::var_os("KATZE_PROBE_FESOBJ") {
                            for name in spec.to_string_lossy().split(',') {
                                for bank in 0..8 {
                                    if let Some(handle) = scene.objects().lookup(name, bank) {
                                        let object = scene.objects().get(handle).unwrap();
                                        let state = &object.state;
                                        let display = match state {
                                            katze_system2::runtime::fes::ObjectState::Plane(p) => {
                                                Some(p.base.display())
                                            }
                                            katze_system2::runtime::fes::ObjectState::CPlane(p) => {
                                                Some(p.display())
                                            }
                                            katze_system2::runtime::fes::ObjectState::String(s) => {
                                                Some(s.layout.base.base.display())
                                            }
                                            katze_system2::runtime::fes::ObjectState::Image(i) => {
                                                Some(i.base.display)
                                            }
                                            _ => None,
                                        };
                                        println!(
                                            "    FES {id} {name}[{bank}] 句柄 {} 状态 {} 显示 {:?}",
                                            object.handle(),
                                            state.kind(),
                                            display
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        if std::env::var_os("KATZE_PROBE_FESBTN").is_some() {
                            for (name, kind) in [
                                ("btn1", 5),
                                ("btn2", 5),
                                ("btn3", 2),
                                ("btn4", 3),
                                ("btn5", 4),
                            ] {
                                for bank in 0..kind {
                                    if let Some(handle) = scene.objects().lookup(name, bank) {
                                        let object = scene.objects().get(handle).unwrap();
                                        if let katze_system2::runtime::fes::ObjectState::Button(
                                            button,
                                        ) = &object.state
                                        {
                                            println!(
                                                "    FES {id} {name}[{bank}] 启用 {}",
                                                button.enabled
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for id in 0..160 {
                if let Some(page) = runtime.object(id).and_then(|o| o.message_page()) {
                    println!(
                        "    消息页 {id}: 文本 {:?} 字符数 {} 剩余 {:?}",
                        page.source(),
                        page.source().chars().count(),
                        page.remaining(u32::MAX)
                    );
                }
            }
            if std::env::var_os("KATZE_PROBE_REGIONS").is_some() {
                for id in 0..160 {
                    if let Some(state) = runtime.input_state(id) {
                        for region in state.regions().records() {
                            let b = region.bounds();
                            println!(
                                "    区域 任务 {id} 编号 {} 启用 {} 选择 {} 事件 {:08x} 范围 {}..{}, {}..{}",
                                region.id(),
                                region.enabled(),
                                region.selection_enabled(),
                                region.event_mask(),
                                b.left,
                                b.right,
                                b.top,
                                b.bottom
                            );
                        }
                    }
                }
            }
            for handle in runtime.cst_handles() {
                if let Some(ctx) = runtime.cst_context(handle) {
                    println!("    CST {handle} 位置 {:?}", ctx.position());
                }
            }
            if let Some(vm) = runtime.object(task).and_then(|object| object.interpreter()) {
                println!("第 {frame} 帧任务 {task} pc {}", vm.pc);
                dump(vm);
            }
            if let Ok(spec) = std::env::var("KATZE_PROBE_MEM") {
                for id in 0..160 {
                    let Some(vm) = runtime.object(id).and_then(|object| object.interpreter())
                    else {
                        continue;
                    };
                    for item in spec.split(',') {
                        let Some((address, length)) = item.split_once(':') else {
                            continue;
                        };
                        let (Ok(address), Ok(length)) =
                            (address.parse::<u32>(), length.parse::<u32>())
                        else {
                            continue;
                        };
                        if let Ok(bytes) = vm.read(address, length as usize) {
                            println!(
                                "  任务 {id} pc {} @{address}: {:?} {:?}",
                                vm.pc,
                                String::from_utf8_lossy(&bytes),
                                &bytes[..bytes.len().min(64)]
                            );
                        }
                    }
                }
            }
            {
                let drawables = runtime.drawables();
                let mut count = 0;
                let mut detail = Vec::new();
                for id in 0..256 {
                    let Some(entry) = drawables.get(id) else {
                        continue;
                    };
                    count += 1;
                    detail.push(format!(
                        "{id} 显示 {} 三角形 {} 画布 {:?} 界限 {:?} 位置 {:?} 色 {:08x}/{:08x} 混合 {} 裁剪 {:?}",
                        entry.display(),
                        entry.triangles().len(),
                        entry.canvas_triangles().map(|t| t.len()),
                        entry.bounds(),
                        entry.transform().position(),
                        entry.color().0,
                        entry.color().1,
                        entry.blend_mode(),
                        entry.clip(),
                    ));
                    if std::env::var_os("KATZE_PROBE_TRIANGLES").is_some() {
                        for (index, triangle) in entry.triangles().iter().take(2).enumerate() {
                            for (vertex_index, vertex) in triangle.iter().enumerate() {
                                detail.push(format!(
                                    "      三角形 {index} 点 {vertex_index} {:?}",
                                    vertex
                                ));
                            }
                        }
                    }
                }
                println!("  绘图 {count} 项");
                for line in detail.iter().take(120) {
                    println!("    {line}");
                }
            }
            if let Some(state) = runtime
                .object(input)
                .and_then(|object| object.input_state())
            {
                for region in state.regions().records().iter().take(24) {
                    println!(
                        "  区域 {} 范围 {:?} 启用 {} 选择 {}",
                        region.id(),
                        region.bounds(),
                        region.enabled(),
                        region.selection_enabled()
                    );
                }
            }
        }
        if frame % 120 == 0 {
            if let Some(vm) = runtime.object(task).and_then(|object| object.interpreter()) {
                println!("第 {frame} 帧 pc {} 栈 {}", vm.pc, vm.stack_bytes().len());
                for id in 0..160 {
                    if let Some(other) = runtime.object(id).and_then(|object| object.interpreter())
                    {
                        if id != task {
                            println!("    任务 {id} pc {}", other.pc);
                        }
                    }
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("KATZE_PROBE_SHOT") {
        if let Ok(spec) = std::env::var("KATZE_PROBE_HIDE") {
            for id in spec
                .split(',')
                .filter_map(|value| value.parse::<u32>().ok())
            {
                runtime.drawables_mut().set_display(id, 0);
            }
        }
        let mut output = katze_system2::graphics::RenderTarget::new([1280, 720])?;
        runtime.render_frame(
            &mut output,
            &katze_system2::graphics::Projection::default(),
            [
                1., 0., 0., 0., //
                0., 1., 0., 0., //
                0., 0., 1., 0., //
                0., 0., 0., 1.,
            ],
            katze_system2::graphics::TextureFilter::Point,
        )?;
        // `KATZE_PROBE_SHOT` 以 "@canvas" 结尾时转储蓄布而非合成帧。
        let path_str = path.to_string_lossy().into_owned();
        let target = if path_str.ends_with("@canvas") {
            runtime.drawing().target.as_ref().ok_or("缺少渲染目标")?
        } else {
            &output
        };
        let [width, height] = target.size();
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        let mut colors = BTreeMap::new();
        for y in 0..height {
            for x in 0..width {
                let value = target.pixel(x, y).unwrap_or(0);
                *colors.entry(value).or_insert(0usize) += 1;
                pixels.extend([
                    ((value >> 16) & 0xff) as u8,
                    ((value >> 8) & 0xff) as u8,
                    (value & 0xff) as u8,
                ]);
            }
        }
        let mut file = std::fs::File::create(&path)?;
        use std::io::Write;
        write!(file, "P6\n{width} {height}\n255\n")?;
        file.write_all(&pixels)?;
        let mut top: Vec<_> = colors.into_iter().collect();
        top.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        println!(
            "画面 {width}x{height} 颜色 {} 主色 {:?}",
            top.len(),
            &top[..top.len().min(5)]
        );
    }
    Ok(())
}
