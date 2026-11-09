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
    input::InputFactory,
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

pub struct Archive {
    file: File,
    cipher: Option<BlowfishLE>,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
}

impl Archive {
    pub fn open(path: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
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

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
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

pub struct Router {
    pub root: String,
    pub config: ArchiveConfig,
    pub sound_config: Option<katze_system2::audio::SoundConfig>,
    pub archives: BTreeMap<String, Archive>,
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
    let mut router = Router {
        root: root.to_string(),
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
            .entries()
            .map(|(name, _, _, _)| name.to_string())
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
    Ok(Game {
        root: root.to_string(),
        router,
        startup,
        start,
        programs,
    })
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
    if let Ok(bytes) = std::fs::read(format!("{root}/config/soundconf.xml")) {
        router.sound_config = Some(katze_system2::audio::SoundConfig::from_xml(&bytes)?);
        runtime.host_mut().sound_config = router.sound_config.clone();
        runtime.host_mut().sound_groups =
            Some(katze_system2::audio::SoundGroups::from_xml(&bytes)?);
        runtime.host_mut().sound_config_enabled = 1;
    }
    let mut settings = katze_system2::settings::SystemSettings::default();
    settings.apply_startup_xml(startup.as_bytes())?;
    let defaults = router.resolve(b"config", b"gamedef")?;
    if let Some(bytes) = router.read(&defaults, ResourceMode::ArchiveFirst)? {
        settings.apply_xml(&bytes)?;
    }
    settings.refresh_sound_volumes();
    runtime.host_mut().languages =
        katze_system2::language::LanguageState::from_xml(startup.as_bytes())?;
    let startup_xml = katze_system2::resource::XmlDocument::parse(startup.as_bytes())?;
    runtime.host_mut().unicode_text = startup_xml.integer(b"APP/utfmode", 0)?;
    runtime.set_resources(router);
    #[cfg(target_os = "macos")]
    runtime.set_fonts(katze_system2::message::macos::MacFonts::new(&root));
    runtime.set_save_services(Saves {
        root: std::env::var("KATZE_SAVE_ROOT").unwrap_or(root),
    });
    runtime.host_mut().message_defaults = Some(Default::default());
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
    runtime.host_mut().system_time_ms = system_time_ms;
    runtime.update(frame, 1)
}
