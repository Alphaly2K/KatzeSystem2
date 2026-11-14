//! 本地探针与窗口示例共用的游戏目录装载和运行时搭建。
//!
//! 归档内部名称按逐条目位移猜测还原：样本归档的校验码不可得时，仍需按名称读取资源。
//! 该猜测只服务于本地工具，不进入库接口。

use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, KeyInit},
};
use katze_system2::{
    Error, Result,
    audio::SoundFactory,
    expression::State,
    graphics::{DrawingContext, GraphicsFactory, RenderTarget},
    input::{ButtonSource, DeviceBinding, DeviceKeys, InputFactory},
    io::{FileSource, SharedSource},
    kcs::{Program, ResourceServices},
    memory::SharedMemory,
    resource::{ArchiveConfig, KifEntrySource, ResourceMode},
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, fes::FesFactory, kind},
    save::SaveServices,
};
use std::{collections::BTreeMap, sync::Arc};

mod configuration;
use configuration::ConfigurationFiles;

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn input_keys(config: &[u8]) -> Result<DeviceKeys> {
    let mut keys = DeviceKeys::from_xml(config)?;
    if keys.bindings().iter().all(|binding| binding.enabled == 0) {
        let mut binding = DeviceBinding {
            enabled: 1,
            ..Default::default()
        };
        binding.codes[ButtonSource::Mouse as usize][0] = 1;
        keys.set_binding(0, binding)?;
    }
    Ok(keys)
}

fn decode(name: &[u8], position_shift: usize) -> Vec<u8> {
    name.iter()
        .enumerate()
        .map(|(i, &b)| decode_byte(b, position_shift + i))
        .collect()
}

fn alphabet_index(byte: u8) -> Option<usize> {
    match byte {
        b'A'..=b'Z' => Some((byte - b'A') as usize),
        b'a'..=b'z' => Some((byte - b'a') as usize + 26),
        _ => None,
    }
}

fn decode_byte(byte: u8, position: usize) -> u8 {
    alphabet_index(byte).map_or(byte, |index| ALPHABET[51 - (index + position) % 52])
}

fn encrypted_name_matches(raw: &[u8], name: &str) -> bool {
    if !name.is_ascii() {
        return (0..52)
            .any(|shift| String::from_utf8_lossy(&decode(raw, shift)).eq_ignore_ascii_case(name));
    }
    if raw.len() != name.len() {
        return false;
    }
    let Some((position, index)) = raw
        .iter()
        .enumerate()
        .find_map(|(position, &byte)| alphabet_index(byte).map(|index| (position, index)))
    else {
        return raw.eq_ignore_ascii_case(name.as_bytes());
    };
    let Some(expected) = alphabet_index(name.as_bytes()[position]) else {
        return false;
    };
    // 忽略大小写时首个字母只对应两种位移；其余字节验证完整名称。
    let shift = (155 - expected - index - position % 52) % 52;
    [shift, (shift + 26) % 52].into_iter().any(|shift| {
        raw.iter()
            .zip(name.bytes())
            .enumerate()
            .all(|(i, (&byte, expected))| {
                decode_byte(byte, shift + i).eq_ignore_ascii_case(&expected)
            })
    })
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

pub struct Archive {
    file: SharedSource,
    key: Option<u32>,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
}

impl Archive {
    pub fn open(path: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let file = Arc::new(FileSource::open(path)?) as SharedSource;
        let mut head = vec![0; 8];
        file.read_exact_at(0, &mut head)?;
        if &head[..4] != b"KIF\0" {
            return Err("KIF 签名无效".into());
        }
        let count = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let mut directory = vec![0; count * 72];
        file.read_exact_at(8, &mut directory)?;
        let encrypted = directory
            .get(..64)
            .is_some_and(|name| name.split(|&b| b == 0).next() == Some(b"__key__.dat"));
        let mut entries = Vec::new();
        let mut archive_key = None;
        if encrypted {
            let length = u32::from_le_bytes(directory[68..72].try_into().unwrap());
            let key = katze_system2::kcs::ScriptRandom::seeded(length).next_integer();
            let value = BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap();
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
            archive_key = Some(key);
        } else {
            for i in 0..count {
                let base = i * 72;
                let name = &directory[base..base + 64];
                let end = name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len());
                entries.push(Entry {
                    name: String::from_utf8_lossy(&name[..end]).into_owned(),
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
            key: archive_key,
            entries,
            index,
        })
    }

    /// 精确名称优先；未命中时逐条目按请求名称反查。
    ///
    /// 样本的校验码不可得，条目名称的位移只能猜测；反查只服务本机探针。
    fn locate(&self, name: &str) -> Option<usize> {
        if let Some(&position) = self.index.get(&name.to_ascii_uppercase()) {
            return Some(position);
        }
        self.key?;
        for (position, entry) in self.entries.iter().enumerate() {
            let end = entry
                .raw
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(entry.raw.len());
            if encrypted_name_matches(&entry.raw[..end], name) {
                return Some(position);
            }
        }
        None
    }

    pub fn open_source(&self, name: &str) -> Result<Option<SharedSource>> {
        let Some(position) = self.locate(name) else {
            return Ok(None);
        };
        let entry = &self.entries[position];
        Ok(Some(Arc::new(KifEntrySource::new(
            self.file.clone(),
            entry.offset as u64,
            entry.length,
            self.key,
        )?)))
    }

    /// 枚举已还原的条目名及原始名称字节；仅服务诊断输出。
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[u8], u32, u32)> {
        self.entries.iter().map(|entry| {
            let end = entry
                .raw
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(entry.raw.len());
            (
                entry.name.as_str(),
                &entry.raw[..end],
                entry.offset,
                entry.length,
            )
        })
    }
}

fn open_file(path: impl AsRef<std::path::Path>) -> Option<SharedSource> {
    FileSource::open(path)
        .ok()
        .map(|source| Arc::new(source) as SharedSource)
}

pub fn read_source(source: SharedSource) -> Result<Vec<u8>> {
    let length =
        usize::try_from(source.len()?).map_err(|_| Error::new(0, "资源大小超出宿主范围"))?;
    let mut bytes = vec![0; length];
    source.read_exact_at(0, &mut bytes)?;
    Ok(bytes)
}

pub struct Router {
    pub root: String,
    pub config: ArchiveConfig,
    pub sound_config: Option<katze_system2::audio::SoundConfig>,
    pub archives: BTreeMap<String, Archive>,
    pub patches: Vec<Archive>,
    pub script_encoding: &'static encoding_rs::Encoding,
}

impl Router {
    fn read_name(&mut self, name: &str, mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.open_name(name, mode)?.map(read_source).transpose()
    }

    fn open_name(&mut self, name: &str, mode: ResourceMode) -> Result<Option<SharedSource>> {
        let inner = name.rsplit('/').next().unwrap_or(name);
        for patch in &mut self.patches {
            if let Some(bytes) = patch.open_source(inner)? {
                return Ok(Some(bytes));
            }
        }
        if mode == ResourceMode::File {
            return Ok(open_file(std::path::Path::new(&self.root).join(name)));
        }
        let (prefix, inner) = name.split_once('/').unwrap_or(("", name));
        for entry in &self.config.entries {
            let entry_prefix = String::from_utf8_lossy(&entry.category.prefix);
            let unqualified = mode == ResourceMode::LooseFirst && prefix.is_empty();
            if !unqualified && !entry_prefix.eq_ignore_ascii_case(prefix) {
                continue;
            }
            let archive = self.archives.get_mut(entry_prefix.as_ref());
            let loose = || {
                let directory = String::from_utf8_lossy(&entry.directory).replace('\\', "/");
                let path = std::path::Path::new(&self.root).join(directory).join(inner);
                open_file(path)
            };
            if mode == ResourceMode::LooseFirst {
                if let Some(bytes) = loose() {
                    return Ok(Some(bytes));
                }
            }
            if let Some(archive) = archive {
                if let Some(bytes) = archive.open_source(inner)? {
                    return Ok(Some(bytes));
                }
            }
            if mode == ResourceMode::ArchiveFirst && !entry.directory.is_empty() {
                if let Some(bytes) = loose() {
                    return Ok(Some(bytes));
                }
            }
        }
        Ok(None)
    }

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
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        self.script_encoding
    }
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

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        let name = String::from_utf8_lossy(name).replace('\\', "/");
        let result = self.read_name(&name, mode);
        if std::env::var_os("KATZE_PROBE_READS").is_some() {
            eprintln!(
                "读取 {name} -> {:?}",
                result.as_ref().map(|value| value.as_ref().map(Vec::len))
            );
        }
        result
    }

    fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<SharedSource>> {
        let name = String::from_utf8_lossy(name).replace('\\', "/");
        self.open_name(&name, mode)
    }

    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        let name = &name[..name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())];
        let name = String::from_utf16(name)
            .map_err(|_| Error::new(0, "资源名称包含无效 UTF-16"))?
            .replace('\\', "/");
        self.read_name(&name, mode)
    }
}

pub struct Programs {
    pub programs: BTreeMap<String, Program>,
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

pub struct Saves {
    pub root: String,
}

impl SaveServices for Saves {
    fn read(&mut self, _: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(name)
            .ok_or_else(|| Error::new(0, "存档名称包含无效 CP932"))?
            .replace('\\', "/");
        Ok(std::fs::read(std::path::Path::new(&self.root).join(name)).ok())
    }

    fn write(&mut self, _: u32, name: &[u8], bytes: &[u8]) -> Result<bool> {
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(name)
            .ok_or_else(|| Error::new(0, "存档名称包含无效 CP932"))?
            .replace('\\', "/");
        let _ = std::fs::create_dir_all(&self.root);
        Ok(std::fs::write(std::path::Path::new(&self.root).join(name), bytes).is_ok())
    }

    fn modified_time(&mut self, _: u32, name: &[u8]) -> Result<Option<u64>> {
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(name)
            .ok_or_else(|| Error::new(0, "存档名称包含无效 CP932"))?
            .replace('\\', "/");
        let Ok(metadata) = std::fs::metadata(std::path::Path::new(&self.root).join(name)) else {
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

    fn modified_date(&mut self, owner: u32, name: &[u8]) -> Result<Option<[u32; 6]>> {
        self.modified_time(owner, name)?
            .map(local_save_date)
            .transpose()
    }
}

fn local_save_date(file_time: u64) -> Result<[u32; 6]> {
    let seconds = (i128::from(file_time) - 116_444_736_000_000_000i128) / 10_000_000;
    let seconds =
        libc::time_t::try_from(seconds).map_err(|_| Error::new(0, "存档修改时间超过平台范围"))?;
    let mut date = std::mem::MaybeUninit::<libc::tm>::uninit();
    #[cfg(unix)]
    let valid = unsafe { !libc::localtime_r(&seconds, date.as_mut_ptr()).is_null() };
    #[cfg(windows)]
    let valid = unsafe { libc::localtime_s(date.as_mut_ptr(), &seconds) == 0 };
    #[cfg(not(any(unix, windows)))]
    let valid = false;
    if !valid {
        return Err(Error::new(0, "存档本地日期转换失败"));
    }
    let date = unsafe { date.assume_init() };
    let values = [
        date.tm_year + 1900,
        date.tm_mon + 1,
        date.tm_mday,
        date.tm_hour,
        date.tm_min,
        date.tm_sec,
    ];
    let mut result = [0; 6];
    for (target, value) in result.iter_mut().zip(values) {
        *target = u32::try_from(value).map_err(|_| Error::new(0, "存档本地日期字段超出范围"))?;
    }
    Ok(result)
}

/// 游戏目录装载结果；`router` 在 `build_runtime` 时被移交资源服务。
pub struct Game {
    pub root: String,
    pub router: Router,
    pub startup: String,
    pub start: String,
    pub programs: BTreeMap<String, Program>,
}

pub fn load_game(root: &str) -> std::result::Result<Game, Box<dyn std::error::Error>> {
    let startup = std::fs::read_to_string(format!("{root}/config/startup.xml"))?;
    let config = ArchiveConfig::from_xml(&startup)?;
    let script_encoding = match std::env::var("KATZE_SCRIPT_ENCODING") {
        Ok(name) => encoding_rs::Encoding::for_label(name.as_bytes()).ok_or("未知脚本文字编码")?,
        Err(_) => encoding_rs::SHIFT_JIS,
    };
    let mut router = Router {
        root: root.to_string(),
        config: config.clone(),
        sound_config: None,
        archives: BTreeMap::new(),
        patches: Vec::new(),
        script_encoding,
    };
    let mut patches = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Some(number) = patch_number(&name) else {
            continue;
        };
        if entry.file_type()?.is_file() {
            patches.push((number, entry.path()));
        }
    }
    patches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, path) in patches {
        router.patches.push(Archive::open(
            path.to_str().ok_or("补丁归档路径包含无效编码")?,
        )?);
    }
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
    let start = startup
        .split("<start>")
        .nth(1)
        .and_then(|rest| rest.split("</start>").next())
        .ok_or("缺少启动脚本")?
        .trim()
        .to_string();
    println!("启动脚本 {start}");
    let mut programs = BTreeMap::new();
    if let Some(archive) = router.archives.get("kcs.int") {
        let mut names: std::collections::BTreeSet<String> = archive
            .entries()
            .map(|(name, _, _, _)| name.to_string())
            .collect();
        for patch in &router.patches {
            names.extend(
                patch
                    .entries()
                    .filter(|(name, _, _, _)| name.ends_with(".kcs"))
                    .map(|(name, _, _, _)| name.to_string()),
            );
        }
        for name in names {
            let Some(data) =
                router.read_name(&format!("kcs.int/{name}"), ResourceMode::ArchiveFirst)?
            else {
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
    Ok(Game {
        root: root.to_string(),
        router,
        startup,
        start,
        programs,
    })
}

fn patch_number(name: &str) -> Option<u32> {
    let number = name.strip_prefix("update")?.strip_suffix(".int")?;
    if number.len() != 2 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

pub type Factory = SoundFactory<
    FesFactory<
        GraphicsFactory<katze_system2::message::MessageFactory<InputFactory<KcsFactory<Programs>>>>,
    >,
>;

/// 以探针同一配置链创建运行时；输入设备由调用方随后绑定。
pub fn build_runtime(
    game: Game,
    surface: [u32; 2],
) -> std::result::Result<Runtime<Factory>, Box<dyn std::error::Error>> {
    let Game {
        root,
        mut router,
        startup,
        programs,
        ..
    } = game;
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
    let startup_xml = katze_system2::resource::XmlDocument::parse(startup.as_bytes())?;
    runtime.host_mut().sound_groups = Some(katze_system2::audio::SoundGroups::from_startup_xml(
        startup.as_bytes(),
    )?);
    let configuration_files = ConfigurationFiles::new(&root, &startup_xml)?;
    let user_settings =
        configuration_files.read(katze_system2::settings::ConfigurationFile::Main)?;
    let sound_name = String::from_utf16(&configuration_files.sound_name)?;
    if !sound_name.is_empty() {
        let name = router.resolve(b"config", sound_name.as_bytes())?;
        if let Some(bytes) = router.read(&name, ResourceMode::LooseFirst)? {
            let user =
                configuration_files.read(katze_system2::settings::ConfigurationFile::Sound)?;
            router.sound_config = Some(
                katze_system2::audio::SoundConfig::from_xml_with_user_settings(
                    &bytes,
                    user.as_deref(),
                )?,
            );
            runtime.host_mut().sound_config = router.sound_config.clone();
            runtime.host_mut().sound_groups =
                Some(katze_system2::audio::SoundGroups::from_xml(&bytes)?);
            runtime.host_mut().sound_config_enabled = 1;
        }
    }
    let mut settings = katze_system2::settings::SystemSettings::default();
    settings.apply_startup_xml(startup.as_bytes())?;
    let mut platform = katze_system2::settings::PlatformSettings::default();
    platform.apply_startup_xml(startup.as_bytes(), |candidate| candidate)?;
    let defaults = router.resolve(b"config", b"gamedef")?;
    if let Some(bytes) = router.read(&defaults, ResourceMode::ArchiveFirst)? {
        settings.apply_game_xml(&bytes)?;
    }
    runtime.host_mut().languages =
        katze_system2::language::LanguageState::from_xml(startup.as_bytes())?;
    let mut keys = input_keys(startup.as_bytes())?;
    if let Some(bytes) = &user_settings {
        platform.apply_user_xml(bytes, |_| false)?;
        settings.apply_xml(bytes)?;
        runtime.host_mut().languages.apply_user_xml(bytes)?;
        keys.apply_user_xml(bytes)?;
    }
    settings.refresh_sound_volumes();
    runtime.host_mut().configuration_keys = Some(keys);
    runtime.host_mut().platform_settings = Some(platform);
    runtime.host_mut().string_table_config = Some(
        katze_system2::language::StringTableConfig::from_xml(startup.as_bytes())?,
    );
    runtime.host_mut().unicode_text = startup_xml.integer(b"APP/utfmode", 0)?;
    runtime.set_resources(router);
    runtime.reload_language_tables(katze_system2::language::LanguageTarget::Interface)?;
    if runtime.host().languages.primary_index as usize
        >= runtime
            .host()
            .string_table_config
            .as_ref()
            .unwrap()
            .prefixes
            .len()
    {
        runtime.reload_language_tables(katze_system2::language::LanguageTarget::MainText)?;
    }
    let mut fonts = katze_system2::message::Fonts::new(&root);
    fonts.discover_system();
    runtime.set_fonts(fonts);
    runtime.set_save_services(Saves {
        root: configuration_files.directory.to_string_lossy().into_owned(),
    });
    runtime.set_configuration_services(configuration_files);
    let mut message_defaults = Default::default();
    settings.apply_message_defaults(&mut message_defaults);
    runtime.host_mut().message_defaults = Some(message_defaults);
    runtime.host_mut().message_time_scale = Some(startup_xml.integer(b"GAME/strframe_frame", 0)?);
    let target = if settings.texture_grid.contains(&0) {
        RenderTarget::new(surface)?
    } else {
        let count = u64::from(settings.texture_grid[0]) * u64::from(settings.texture_grid[1]);
        if count > 1_000_000 {
            return Err("画布面块数量超过上限".into());
        }
        let mut target = RenderTarget::from_tiles(
            settings.texture_size,
            settings.texture_grid,
            vec![None; count as usize],
        )?;
        target.prepare_tiles(&settings.texture_areas, settings.texture_16bit_row)?;
        target
    };
    runtime.host_mut().system_settings = Some(settings);
    runtime.host_mut().surface_size = surface;
    *runtime.drawing_mut() = DrawingContext {
        mode: 0,
        enabled: true,
        active: true,
        target: Some(target),
    };
    runtime.set_frame_view(
        Default::default(),
        [
            1., 0., 0., 0., //
            0., 1., 0., 0., //
            0., 0., 1., 0., //
            0., 0., 0., 1.,
        ],
        katze_system2::graphics::TextureFilter::Point,
    );
    Ok(runtime)
}

/// 每轮推进一个 1/60 秒逻辑帧，系统毫秒时钟由调用方独立提供。
pub fn advance_frame(
    runtime: &mut Runtime<Factory>,
    frame: u32,
    system_time_ms: u32,
) -> Result<bool> {
    for request in std::mem::take(&mut runtime.host_mut().language_change_requests) {
        runtime.refresh_language(request.target)?;
    }
    runtime.host_mut().system_time_ms = system_time_ms;
    runtime.update(frame, 1)
}
