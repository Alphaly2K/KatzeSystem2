use super::{AudioStats, ProbeWindow, ScriptedDevice, input::StoryReplay, write_shot};
use crate::support::{self, prepare_game};
use katze_system2::{
    host::{
        graphics::software::SoftwareBackend,
        resources::{ResourceMode, ResourceServices},
    },
    runtime::{GameTasks, TaskRuntime},
    session::{FrameOutcome, FrameTime},
};

pub(crate) fn inspect_runtime(
    mut game: support::Game,
    frames: u32,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let programs = support::inspect_programs(&mut game.router)?;
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
                let Some(source) = archive.open_source(&name)? else {
                    continue;
                };
                let data = support::read_source(source)?;
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

    let diagnostics = support::diagnostics::Output::from_env(game.router.script_encoding);
    let entry = game.start.clone();
    let (mut runtime, keys, surface) = prepare_game(game)?;
    runtime.set_diagnostic_trace(diagnostics.trace());
    if let Some(trace) = diagnostics.native_trace() {
        runtime.set_native_trace(trace);
    }
    let mut replay = StoryReplay::from_env(surface);
    let mut audio_stats = std::env::var_os("KATZE_PROBE_AUDIO_STATS")
        .is_some()
        .then(AudioStats::default);
    runtime.set_input_device(ScriptedDevice::new(keys, replay.as_ref(), surface))?;
    runtime.validate_startup()?;
    let fullscreen = runtime
        .window_preferences()
        .is_some_and(|prefs| prefs.fullscreen);
    let (
        mut runtime,
        GameTasks {
            input,
            sound,
            entry: task,
        },
    ) = runtime
        .into_task_runtime()
        .start_game_with_tasks(entry.as_bytes())?;
    let mut window = ProbeWindow { fullscreen };
    let mut mixer = std::env::var_os("KATZE_PROBE_DETACHED_MIXER")
        .map(|_| runtime.connect_audio_output())
        .transpose()?;
    let mut sound_trace = super::SoundTrace::from_env()?;
    println!("输入 {input} 声音 {sound}");
    println!("主任务 {task}");
    let dump = |vm: &katze_system2::kcs::Interpreter| {
        let instructions = vm.program().instructions();
        if let Ok(bytes) = vm.read(2200, 300) {
            for (offset, chunk) in bytes.chunks(64).enumerate() {
                println!(
                    "    {:4}: {:?}",
                    2200 + offset * 64,
                    String::from_utf8_lossy(chunk)
                );
            }
        }
        for instruction in instructions.iter().filter(|instruction| {
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
        let position = instructions.partition_point(|item| item.offset < vm.pc);
        println!("    pc 索引 {position}");
        for instruction in
            &instructions[position.saturating_sub(2)..(position + 3).min(instructions.len())]
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
    let shared_range = std::env::var("KATZE_PROBE_SHARED")
        .ok()
        .map(
            |spec| -> std::result::Result<_, Box<dyn std::error::Error>> {
                let (start, count) = spec
                    .split_once(':')
                    .ok_or("KATZE_PROBE_SHARED expects start:count")?;
                let start = start.parse::<usize>()?;
                let end = start
                    .checked_add(count.parse::<usize>()?)
                    .ok_or("KATZE_PROBE_SHARED range overflow")?;
                let range = start..end;
                if runtime.numbers().shared.get(range.clone()).is_none() {
                    return Err("KATZE_PROBE_SHARED range exceeds script storage".into());
                }
                Ok(range)
            },
        )
        .transpose()?;
    let mut now = 0;
    let mut failure = None;
    let ticks = if replay.is_some() { 3 } else { 1 };
    for frame in 0..frames {
        now += ticks;
        let system_time_ms = (u64::from(now) * 1000 / 60) as u32;
        let started = diagnostics.timing.then(std::time::Instant::now);
        let advancement = runtime.advance(FrameTime {
            tick: now,
            increment: 1,
            system_time_ms,
        });
        diagnostics.report(runtime.take_script_messages());
        if let Err(error) = advancement {
            println!("第 {frame} 帧 pc {}：{}", error.offset, error.message);
            for id in task_ids(&runtime) {
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
            if replay.is_some() {
                return Err(error.into());
            }
            failure = Some(error);
            break;
        }
        if matches!(advancement, Ok(FrameOutcome::Exited))
            || runtime.process_window_requests(&mut window)?
        {
            break;
        }
        if let Some(started) = started {
            let elapsed = started.elapsed();
            if elapsed.as_millis() >= 20 {
                eprintln!(
                    "[slow-update] frame={frame} ms={:.2}",
                    elapsed.as_secs_f64() * 1000.
                );
            }
        }
        let mut buffer = [0.; 4800];
        let pcm = &mut buffer[..(ticks as usize * 800 * 2)];
        if let Some(mixer) = mixer.as_mut().filter(|_| runtime.object(sound).is_some()) {
            mixer.render(48_000, pcm)?;
        } else {
            runtime.mix_audio_stereo(48_000, pcm)?;
        }
        if let Some(stats) = &mut audio_stats {
            stats.add(frame, pcm);
        }
        if let Some(trace) = &mut sound_trace {
            if let Some(state) = runtime
                .object_mut(sound)
                .and_then(|object| object.audio_state_mut())
            {
                trace.update(frame, state)?;
            }
        }
        if let Some(replay) = &mut replay {
            replay.observe_movie(runtime.has_exclusive_movie());
            if let Some(state) = runtime.input_state(input) {
                replay.update(frame, state.regions().records());
            }
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
            if let Some(range) = &shared_range {
                println!(
                    "SHARED frame={frame} range={range:?} values={:?}",
                    &runtime.numbers().shared[range.clone()]
                );
            }
            for id in task_ids(&runtime) {
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
                            let names = scene.interpreter().named_values();
                            for (name, slots) in names.iter() {
                                let name = std::str::from_utf8(name.as_bytes()).unwrap_or("");
                                if fes_names.to_string_lossy().split(',').any(|f| name == f) {
                                    println!("    FES {id} 命名 {name} = {slots:?}");
                                }
                            }
                        }
                        if let Some(spec) = std::env::var_os("KATZE_PROBE_FESOBJ") {
                            let spec = spec.to_string_lossy();
                            let handles: Vec<_> = if spec == "*" {
                                scene.objects().update_order().collect()
                            } else {
                                spec.split(',')
                                    .filter_map(|name| {
                                        (0..8).find_map(|bank| scene.objects().lookup(name, bank))
                                    })
                                    .collect()
                            };
                            for handle in handles {
                                let object = scene.objects().get(handle).unwrap();
                                let state = &object.state;
                                let name = object.name();
                                let bank = object.bank();
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
                                if let katze_system2::runtime::fes::ObjectState::Image(image) =
                                    state
                                {
                                    println!(
                                        "      文件 {:?} 帧 {} 透明度 {} 掩码 {} 图像 {:?} 属性 {:?}",
                                        String::from_utf8_lossy(&image.file),
                                        image.image_number,
                                        image.base.blend,
                                        image.base.mask,
                                        object.image_handle(),
                                        image.base,
                                    );
                                }
                            }
                        }
                        if std::env::var_os("KATZE_PROBE_FESBTN").is_some() {
                            for handle in scene.objects().update_order() {
                                let object = scene.objects().get(handle).unwrap();
                                if let katze_system2::runtime::fes::ObjectState::Button(button) =
                                    &object.state
                                {
                                    println!(
                                        "    FES {id} {}[{}] 启用 {} 按钮任务 {:?}",
                                        object.name(),
                                        object.bank(),
                                        button.enabled,
                                        object.task_id()
                                    );
                                }
                            }
                        }
                    }
                }
            }
            for id in task_ids(&runtime) {
                if let Some(page) = runtime.object(id).and_then(|o| o.message_page()) {
                    println!(
                        "    消息页 {id}: 文本 {:?} 字符数 {} 剩余 {:?} 显示时间 {:?}",
                        page.source(),
                        page.source().chars().count(),
                        page.remaining(u32::MAX),
                        page.schedule().deadlines
                    );
                }
            }
            if std::env::var_os("KATZE_PROBE_REGIONS").is_some() {
                for id in task_ids(&runtime) {
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
                    println!("    CST {handle} 默认读取位置 {:?}", ctx.position());
                }
            }
            if let Some(vm) = runtime.object(task).and_then(|object| object.interpreter()) {
                println!("第 {frame} 帧任务 {task} pc {}", vm.pc);
                dump(vm);
            }
            if let Ok(spec) = std::env::var("KATZE_PROBE_MEM") {
                for id in task_ids(&runtime) {
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
                let ids = runtime.drawables_mut().order().to_vec();
                let drawables = runtime.drawables();
                let mut count = 0;
                let mut detail = Vec::new();
                for id in ids {
                    let Some(entry) = drawables.get(id) else {
                        continue;
                    };
                    count += 1;
                    detail.push(format!(
                        "{id} 显示 {} 三角形 {} 画布 {:?} 界限 {:?} 位置 {:?} 色 {:08x}/{:08x} 混合 {} 裁剪 {:?} 比例 {:?} 基点 {:?} 相机 {} 投影原点 {} 源变换 {:?}",
                        entry.display(),
                        entry.triangles().len(),
                        entry.canvas_triangles().map(|t| t.len()),
                        entry.bounds(),
                        entry.transform().position(),
                        entry.color().0,
                        entry.color().1,
                        entry.blend_mode(),
                        entry.clip(),
                        entry.transform().scale(),
                        entry.transform().base_point(),
                        entry.camera_id(),
                        entry.projection_origin_mode(),
                        entry.geometry_transform(true).map(|source| (source.base_point(), source.position(), source.scale())),
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
                for id in task_ids(&runtime) {
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
    if let Some(stats) = &audio_stats {
        stats.report();
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
        let mut output = katze_system2::graphics::RenderTarget::new(surface)?;
        runtime
            .with_frame(|draw, canvas| SoftwareBackend::render_into(draw, canvas, &mut output))?;
        // `KATZE_PROBE_SHOT` 以 "@canvas" 结尾时转储蓄布而非合成帧。
        let path_str = path.to_string_lossy().into_owned();
        let target = if path_str.ends_with("@canvas") {
            runtime.drawing().target.as_ref().ok_or("缺少渲染目标")?
        } else {
            &output
        };
        write_shot(&path, target.size(), |x, y| target.pixel(x, y))?;
    }
    failure.map_or(Ok(()), |error| Err(error.into()))
}

fn task_ids(runtime: &TaskRuntime) -> Vec<u32> {
    let root = runtime.tree().root_id();
    std::iter::once(root)
        .chain(runtime.tree().descendants(root).unwrap())
        .collect()
}
