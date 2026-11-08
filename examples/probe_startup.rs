//! 用本机游戏目录跑启动脚本，报告首个未接入的宿主入口。
//!
//! 归档内部名称按逐条目位移猜测还原：样本归档的校验码不可得时，仍需按名称读取资源。
//! 该猜测只服务于本地探针，不进入库接口。用法：`cargo run --example probe_startup -- <游戏目录>`。

use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, KeyInit},
};
use katze_system2::{
    Error, Result,
    audio::SoundFactory,
    expression::State,
    graphics::{DrawingContext, GraphicsFactory, RenderTarget},
    input::{InputDevice, InputFactory, KeySample, PointerSample},
    kcs::{Program, ResourceServices},
    memory::SharedMemory,
    resource::{ArchiveConfig, ResourceMode},
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, fes::FesFactory, kind},
    save::SaveServices,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn decode(name: &[u8], position_shift: usize) -> Vec<u8> {
    name.iter()
        .enumerate()
        .map(|(i, &b)| match ALPHABET.iter().position(|c| *c == b) {
            Some(index) => ALPHABET[51 - (index + position_shift + i) % 52],
            None => b,
        })
        .collect()
}

fn name_score(name: &[u8]) -> i32 {
    let mut score = 0;
    for &b in name {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'/' | b'\\') {
            score += 2;
        } else if b.is_ascii_uppercase() {
            score -= 2;
        } else {
            score -= 8;
        }
    }
    for extension in [
        b".kcs".as_slice(),
        b".cst",
        b".hg3",
        b".ogg",
        b".xml",
        b".dat",
        b".anm",
    ] {
        if name.ends_with(extension) {
            score += 6;
        }
    }
    score
}

fn guess_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let raw = &raw[..end];
    let mut best = (i32::MIN, Vec::new());
    for position in 0..52 {
        let name = decode(raw, position);
        let score = name_score(&name);
        if score > best.0 {
            best = (score, name);
        }
    }
    String::from_utf8_lossy(&best.1).into_owned()
}

struct Entry {
    name: String,
    raw: Vec<u8>,
    offset: u32,
    length: u32,
}

struct Archive {
    file: File,
    cipher: Option<BlowfishLE>,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
}

impl Archive {
    fn open(path: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(path)?;
        let mut head = vec![0; 8];
        file.read_exact(&mut head)?;
        if &head[..4] != b"KIF\0" {
            return Err("KIF 签名无效".into());
        }
        let count = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let mut directory = vec![0; count * 72];
        file.read_exact(&mut directory)?;
        let encrypted = directory[..64].split(|&b| b == 0).next() == Some(b"__key__.dat");
        let mut entries = Vec::new();
        let mut cipher = None;
        if encrypted {
            let length = u32::from_le_bytes(directory[68..72].try_into().unwrap());
            let key = katze_system2::kcs::ScriptRandom::seeded(length)
                .next_integer()
                .to_le_bytes();
            let value = BlowfishLE::new_from_slice(&key).unwrap();
            for i in 1..count {
                let base = i * 72;
                let mut block = [0; 8];
                let offset =
                    u32::from_le_bytes(directory[base + 64..base + 68].try_into().unwrap())
                        .wrapping_add(i as u32);
                block[..4].copy_from_slice(&offset.to_le_bytes());
                block[4..].copy_from_slice(&directory[base + 68..base + 72]);
                value.decrypt_block((&mut block).into());
                let offset = u32::from_le_bytes(block[..4].try_into().unwrap());
                entries.push(Entry {
                    name: guess_name(&directory[base..base + 64]),
                    raw: directory[base..base + 64].to_vec(),
                    offset,
                    length: u32::from_le_bytes(block[4..].try_into().unwrap()),
                });
            }
            cipher = Some(value);
        } else {
            for i in 0..count {
                let base = i * 72;
                entries.push(Entry {
                    name: guess_name(&directory[base..base + 64]),
                    raw: directory[base..base + 64].to_vec(),
                    offset: u32::from_le_bytes(directory[base + 64..base + 68].try_into().unwrap()),
                    length: u32::from_le_bytes(directory[base + 68..base + 72].try_into().unwrap()),
                });
            }
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.name.to_ascii_uppercase(), i))
            .collect();
        Ok(Self {
            file,
            cipher,
            entries,
            index,
        })
    }

    /// 精确名称优先；未命中时逐条目尝试全部位移，按请求名称反查。
    ///
    /// 样本的校验码不可得，条目名称的位移只能猜测；反查只服务本机探针。
    fn locate(&self, name: &str) -> Option<usize> {
        if let Some(&position) = self.index.get(&name.to_ascii_uppercase()) {
            return Some(position);
        }
        for (position, entry) in self.entries.iter().enumerate() {
            let end = entry
                .raw
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(entry.raw.len());
            for shift in 0..52 {
                let decoded = decode(&entry.raw[..end], shift);
                if String::from_utf8_lossy(&decoded).eq_ignore_ascii_case(name) {
                    return Some(position);
                }
            }
        }
        None
    }

    fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(position) = self.locate(name) else {
            return Ok(None);
        };
        let entry = &self.entries[position];
        self.file
            .seek(SeekFrom::Start(entry.offset as u64))
            .map_err(|error| Error::new(0, format!("归档读取失败：{error}")))?;
        let mut data = vec![0; entry.length as usize];
        self.file
            .read_exact(&mut data)
            .map_err(|error| Error::new(0, format!("归档读取失败：{error}")))?;
        if let Some(cipher) = &self.cipher {
            for block in data.chunks_mut(8).filter(|block| block.len() == 8) {
                cipher.decrypt_block((&mut *block).into());
            }
        }
        Ok(Some(data))
    }
}

struct Router {
    root: String,
    config: ArchiveConfig,
    sound_config: Option<katze_system2::audio::SoundConfig>,
    archives: BTreeMap<String, Archive>,
}

impl Router {
    /// 新声音配置：文件名先按分类树的 `<head>` 前缀找节点，再取其 `<int>` 归档类别。
    fn sound_archive(&self, name: &[u8]) -> Option<Vec<u8>> {
        let part = self.sound_config.as_ref()?.file_part(name).ok().flatten()?;
        if part.archive.is_empty() {
            None
        } else {
            Some(part.archive.clone())
        }
    }
}

impl ResourceServices for Router {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        let value = if category == b"sound" {
            match self.sound_archive(name) {
                Some(archive) => self.config.resolve(&archive, name),
                // 未命中分类节点时名称原样传递，读取结果按缺失资源处理。
                None => katze_system2::resource::Category::default().resolve(name),
            }
        } else {
            self.config.resolve(category, name)
        };
        if std::env::var_os("KATZE_PROBE_TRACE").is_some() {
            eprintln!(
                "解析 类别={:?} 名称={:?} -> {:?}",
                String::from_utf8_lossy(category),
                String::from_utf8_lossy(name),
                value
                    .as_ref()
                    .map(|v| String::from_utf8_lossy(v).into_owned())
            );
        }
        value
    }

    fn resolve_slot(&mut self, slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        self.config.resolve_slot(slot, name)
    }

    fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        let name = String::from_utf8_lossy(name).into_owned();
        if std::env::var_os("KATZE_PROBE_READS").is_some() {
            let result = if let Some((prefix, inner)) = name.split_once('/') {
                if let Some(archive) = self.archives.get_mut(prefix) {
                    archive.read(inner)
                } else {
                    Ok(std::fs::read(format!("{}/{}", self.root, name)).ok())
                }
            } else {
                Ok(std::fs::read(format!("{}/{}", self.root, name)).ok())
            };
            eprintln!(
                "读取 {name} -> {:?}",
                result.as_ref().map(|value| value.as_ref().map(Vec::len))
            );
            return result;
        }
        if let Some((prefix, inner)) = name.split_once('/') {
            if let Some(archive) = self.archives.get_mut(prefix) {
                return archive.read(inner);
            }
        }
        Ok(std::fs::read(format!("{}/{}", self.root, name)).ok())
    }
}

struct Programs {
    programs: BTreeMap<String, Program>,
}

impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        // 装载命令收到补全后的归档名；先按原名再按末段查预载脚本。
        let full = String::from_utf8_lossy(name).to_ascii_lowercase();
        if let Some(program) = self.programs.get(&full) {
            return Ok(Some(program.clone()));
        }
        let key = full.rsplit(['/', '\\']).next().unwrap_or(&full).to_string();
        Ok(self.programs.get(&key).cloned())
    }
}

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

struct Saves {
    root: String,
}

impl SaveServices for Saves {
    fn read(&mut self, _: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
        let name = String::from_utf8_lossy(name).into_owned();
        for directory in ["save", ""] {
            if let Ok(bytes) = std::fs::read(format!("{}/{directory}/{name}", self.root)) {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    fn modified_time(&mut self, _: u32, name: &[u8]) -> Result<Option<u64>> {
        let name = String::from_utf8_lossy(name).into_owned();
        let Ok(metadata) = std::fs::metadata(format!("{}/save/{name}", self.root)) else {
            return Ok(None);
        };
        let Ok(modified) = metadata.modified() else {
            return Ok(None);
        };
        let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) else {
            return Ok(None);
        };
        Ok(Some(
            elapsed.as_nanos() as u64 / 100 + 116_444_736_000_000_000,
        ))
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：probe_startup <游戏目录> [帧数]")?;
    let frames: u32 = std::env::args()
        .nth(2)
        .map_or(Ok(600), |value| value.parse())?;
    let startup = std::fs::read_to_string(format!("{root}/config/startup.xml"))?;
    let config = ArchiveConfig::from_xml(&startup)?;
    let mut router = Router {
        root: root.clone(),
        config: config.clone(),
        sound_config: None,
        archives: BTreeMap::new(),
    };
    for entry in config.entries.iter() {
        let prefix = String::from_utf8_lossy(&entry.category.prefix).into_owned();
        let path = format!("{root}/{prefix}");
        if prefix.is_empty() || !std::path::Path::new(&path).exists() {
            continue;
        }
        match Archive::open(&path) {
            Ok(archive) => {
                router.archives.insert(prefix, archive);
            }
            Err(error) => eprintln!("跳过 {prefix}：{error}"),
        }
    }
    println!("归档 {}", router.archives.len());
    if let Some(name) = std::env::var_os("KATZE_PROBE_LIST") {
        let name = name.to_string_lossy().into_owned();
        if let Some(archive) = router.archives.get(&name) {
            for entry in archive.entries.iter() {
                let end = entry
                    .raw
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(entry.raw.len());
                println!(
                    "{} raw={:?} {} {}",
                    entry.name,
                    String::from_utf8_lossy(&entry.raw[..end]),
                    entry.offset,
                    entry.length
                );
            }
        }
    }
    if let Some(name) = std::env::var_os("KATZE_PROBE_PRINT") {
        let name = name.to_string_lossy().into_owned();
        let data = router.read(name.as_bytes(), ResourceMode::LooseFirst)?;
        match data {
            Some(data) => {
                println!("打印 {name} 共 {} 字节", data.len());
                println!("{}", String::from_utf8_lossy(&data[..data.len().min(600)]));
            }
            None => println!("打印 {name} 未找到"),
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
        let keys: Vec<String> = router.archives.keys().cloned().collect();
        for key in keys {
            if !prefix.is_empty() && key != prefix {
                continue;
            }
            let archive = router.archives.get_mut(&key).unwrap();
            let names: Vec<String> = archive
                .entries
                .iter()
                .map(|entry| entry.name.clone())
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

    let start = startup
        .split("<start>")
        .nth(1)
        .and_then(|rest| rest.split("</start>").next())
        .ok_or("缺少启动脚本")?
        .trim()
        .to_string();
    println!("启动脚本 {start}");
    let mut programs = BTreeMap::new();
    if let Some(archive) = router.archives.get_mut("kcs.int") {
        let names: Vec<String> = archive
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        for name in names {
            let Some(data) = archive.read(&name)? else {
                continue;
            };
            let Ok(program) = Program::parse(&data) else {
                continue;
            };
            programs.insert(name.to_ascii_lowercase(), program.clone());
            programs.insert(name.trim_end_matches(".kcs").to_ascii_lowercase(), program);
        }
    }
    println!("脚本 {}", programs.len());
    if let Some(spec) = std::env::var_os("KATZE_PROBE_FES") {
        let spec = spec.to_string_lossy().into_owned();
        let data = router.read(spec.as_bytes(), ResourceMode::LooseFirst)?;
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
    let start_file = start
        .rsplit('/')
        .next()
        .unwrap_or(&start)
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

    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 2)),
        State::new(1 << 20),
        SoundFactory {
            enabled: true,
            fallback: FesFactory {
                fallback: GraphicsFactory {
                    fallback: katze_system2::message::MessageFactory {
                        fallback: InputFactory {
                            fallback: KcsFactory {
                                programs: Programs { programs },
                                external: SharedMemory::new(vec![0; 16 << 20])?,
                            },
                        },
                    },
                },
            },
        },
    );
    if let Ok(bytes) = std::fs::read(format!("{root}/config/soundconf.xml")) {
        router.sound_config = Some(katze_system2::audio::SoundConfig::from_xml(&bytes)?);
        runtime.host_mut().sound_config = router.sound_config.clone();
        runtime.host_mut().sound_groups =
            Some(katze_system2::audio::SoundGroups::from_xml(&bytes)?);
        runtime.host_mut().sound_config_enabled = 1;
    }
    runtime.host_mut().languages =
        katze_system2::language::LanguageState::from_xml(startup.as_bytes())?;
    runtime.set_resources(router);
    runtime.set_input_device(ScriptedDevice::new())?;
    runtime.set_save_services(Saves { root: root.clone() });
    runtime.host_mut().message_defaults = Some(Default::default());
    #[cfg(target_os = "macos")]
    runtime.set_fonts(katze_system2::message::macos::MacFonts::new(&root));
    runtime.host_mut().system_settings = Some(Default::default());
    runtime.host_mut().surface_size = [1280, 720];
    *runtime.drawing_mut() = DrawingContext {
        mode: 0,
        enabled: true,
        active: true,
        target: Some(RenderTarget::new([1280, 720])?),
    };
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
                    .map(|operand| (operand.flags, operand.payload))
                    .collect::<Vec<_>>()
            );
        }
    };
    let mut now = 0;
    for frame in 0..frames {
        now += 16;
        if let Err(error) = runtime.update(now, 16) {
            println!("第 {frame} 帧 pc {}：{}", error.offset, error.message);
            for id in 0..64 {
                if let Some(vm) = runtime.object(id).and_then(|object| object.interpreter()) {
                    println!("  任务 {id} pc {}", vm.pc);
                    if id == task && std::env::var_os("KATZE_PROBE_DUMP").is_some() {
                        dump(vm);
                    }
                }
            }
            break;
        }
        let dump_frame = std::env::var("KATZE_PROBE_DUMP")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(200);
        if frame == dump_frame && std::env::var_os("KATZE_PROBE_DUMP").is_some() {
            for id in 0..64 {
                if let Some(node) = runtime.tree().node(id) {
                    let pc = runtime
                        .object(id)
                        .and_then(|object| object.interpreter())
                        .map(|vm| vm.pc);
                    println!(
                        "  节点 {id} 类型 {} 父 {:?} pc {:?}",
                        node.kind, node.parent, pc
                    );
                }
            }
            if let Some(vm) = runtime.object(task).and_then(|object| object.interpreter()) {
                println!("第 {frame} 帧任务 {task} pc {}", vm.pc);
                dump(vm);
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
            }
        }
    }
    if let Some(path) = std::env::var_os("KATZE_PROBE_SHOT") {
        let target = runtime.drawing().target.as_ref().ok_or("缺少渲染目标")?;
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
