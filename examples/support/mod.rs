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
    io::SharedSource,
    kcs::{LocalDateTime, Program, ResourceServices, SystemServices},
    memory::SharedMemory,
    resource::{ArchiveConfig, KifEntrySource, ResourceMode},
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, fes::FesFactory, kind},
    save::SaveServices,
};
use std::{collections::BTreeMap, sync::Arc};

mod configuration;
use configuration::ConfigurationFiles;
mod file_source;
use file_source::FileSource;
mod font_sources;
pub use font_sources::load_fonts;
// The desktop example uses the picker; the offline probe only uses preselection.
#[allow(dead_code)]
pub mod image_import;

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

fn fallback_name_key(name: &[u8], encoded: bool) -> Vec<u8> {
    let mut anchor = None;
    name.iter()
        .copied()
        .enumerate()
        .map(|(position, byte)| {
            let Some(index) = alphabet_index(byte) else {
                return byte;
            };
            let index = if encoded {
                51 - (index + position) % 52
            } else {
                index
            } % 26;
            // 相对首字母的位移同时消去未知密钥位移和大小写差异。
            b'A' + ((26 + index - *anchor.get_or_insert(index)) % 26) as u8
        })
        .collect()
}

fn archive_fallback_key(raw: &[u8]) -> Vec<u8> {
    // 位移以原始字节为单位；替换无效 UTF-8 字节应在消除位移之后进行。
    let key = fallback_name_key(raw, true);
    match String::from_utf8_lossy(&key) {
        std::borrow::Cow::Borrowed(_) => key,
        std::borrow::Cow::Owned(value) => value.into_bytes(),
    }
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

impl Entry {
    fn name_bytes(&self) -> &[u8] {
        self.raw.split(|&byte| byte == 0).next().unwrap()
    }
}

pub struct Archive {
    file: SharedSource,
    generation: Arc<()>,
    key: Option<u32>,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
    fallback_index: BTreeMap<Vec<u8>, usize>,
}

impl Archive {
    pub fn open(path: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Self::from_source(Arc::new(FileSource::open(path)?))
    }

    fn from_source(file: SharedSource) -> std::result::Result<Self, Box<dyn std::error::Error>> {
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
        let mut fallback_index = BTreeMap::new();
        if encrypted {
            for (position, entry) in entries.iter().enumerate() {
                fallback_index
                    .entry(archive_fallback_key(entry.name_bytes()))
                    .or_insert(position);
            }
        }
        Ok(Self {
            file,
            generation: Arc::new(()),
            key: archive_key,
            entries,
            index,
            fallback_index,
        })
    }

    /// 精确名称优先；未命中时按请求名称反查。
    ///
    /// 样本的校验码不可得，条目名称的位移只能猜测；反查只服务本机探针。
    fn locate(&self, name: &str) -> Option<usize> {
        if let Some(&position) = self.index.get(&name.to_ascii_uppercase()) {
            return Some(position);
        }
        self.key?;
        self.fallback_index
            .get(&fallback_name_key(name.as_bytes(), false))
            .copied()
    }

    pub fn open_source(&self, name: &str) -> Result<Option<SharedSource>> {
        let Some(position) = self.locate(name) else {
            return Ok(None);
        };
        let entry = &self.entries[position];
        let source = KifEntrySource::new(
            self.file.clone(),
            entry.offset as u64,
            entry.length,
            self.key,
        )?;
        let snapshot = katze_system2::io::ReadSnapshot::new(self.generation.clone(), position);
        Ok(Some(Arc::new(source.with_snapshot(snapshot))))
    }

    /// 枚举已还原的条目名及原始名称字节；仅服务诊断输出。
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[u8], u32, u32)> {
        self.entries.iter().map(|entry| {
            (
                entry.name.as_str(),
                entry.name_bytes(),
                entry.offset,
                entry.length,
            )
        })
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use blowfish::cipher::BlockEncrypt;

    #[test]
    fn indexed_archive_lookup_matches_exhaustive_decode_and_preserves_priority() {
        let raw = [
            decode(b"A_07/bCd.Hg3", 14),
            decode(b"a_07/BcD.hg3", 5),
            decode(b"A_08/bCd.Hg3", 14),
            b"314.159".to_vec(),
            decode(b"\xc3\xa9.XyZ", 3),
            decode(b"\xffA\xe7\x8cB.ogg", 11),
            decode(b"\xef\xbf\xbdA\xef\xbf\xbdB.ogg", 7),
            decode(b"\xe7\x8c\xab.ogg", 19),
            b"\xff\xff".to_vec(),
        ];
        let mut data = vec![0; 8 + (raw.len() + 1) * 72];
        let seed = 23u32;
        data[..4].copy_from_slice(b"KIF\0");
        data[4..8].copy_from_slice(&((raw.len() + 1) as u32).to_le_bytes());
        data[8..19].copy_from_slice(b"__key__.dat");
        data[76..80].copy_from_slice(&seed.to_le_bytes());
        let key = katze_system2::kcs::ScriptRandom::seeded(seed).next_integer();
        let cipher = BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap();
        for (entry, encrypted_name) in data[80..].chunks_exact_mut(72).zip(&raw) {
            entry[..encrypted_name.len()].copy_from_slice(encrypted_name);
            let block: &mut [u8; 8] = (&mut entry[64..72]).try_into().unwrap();
            cipher.encrypt_block(block.into());
        }
        let archive = Archive::from_source(Arc::new(data)).unwrap();
        let mut requests = vec![
            "missing.hg3".to_string(),
            "314.159".to_string(),
            "\u{732b}.missing".to_string(),
            "\u{fffd}".to_string(),
        ];
        for encrypted_name in &raw {
            for shift in 0..52 {
                let bytes = decode(encrypted_name, shift);
                let name = String::from_utf8_lossy(&bytes);
                requests.extend([name.to_ascii_uppercase(), name.to_ascii_lowercase()]);
            }
        }
        for name in requests {
            let expected = archive
                .entries
                .iter()
                .rposition(|entry| entry.name.eq_ignore_ascii_case(&name))
                .or_else(|| {
                    raw.iter().position(|entry| {
                        (0..52).any(|shift| {
                            String::from_utf8_lossy(&decode(entry, shift))
                                .eq_ignore_ascii_case(&name)
                        })
                    })
                });
            assert_eq!(archive.locate(&name), expected, "{name}");
        }
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
    fn resolve_file(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.config.resolve_file(category, name)
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
        self.open_utf16(name, mode)?.map(read_source).transpose()
    }

    fn open_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<SharedSource>> {
        let name = name.split(|&unit| unit == 0).next().unwrap_or_default();
        let name = String::from_utf16(name)
            .map_err(|_| Error::new(0, "资源名称包含无效 UTF-16"))?
            .replace('\\', "/");
        self.open_name(&name, mode)
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

impl Saves {
    fn path(&self, name: &[u8]) -> Result<std::path::PathBuf> {
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(name)
            .ok_or_else(|| Error::new(0, "存档名称包含无效 CP932"))?
            .replace('\\', "/");
        Ok(std::path::Path::new(&self.root).join(name))
    }

    fn write_file(&self, name: &[u8], bytes: &[u8], preserve_time: bool) -> Result<bool> {
        use std::io::Write;

        let path = self.path(name)?;
        let modified = if preserve_time {
            std::fs::File::open(&path)
                .ok()
                .and_then(|file| file.metadata().ok()?.modified().ok())
        } else {
            None
        };
        let _ = std::fs::create_dir_all(&self.root);
        let Ok(mut file) = std::fs::File::create(&path) else {
            return Ok(false);
        };
        if file.write_all(bytes).is_err() {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Ok(false);
        }
        if let Some(modified) = modified {
            // 原写入器忽略恢复时间的失败，返回值仍取决于完整写入。
            let _ = file.set_times(std::fs::FileTimes::new().set_modified(modified));
        }
        Ok(true)
    }
}

pub struct Files {
    pub root: String,
}

impl katze_system2::io::FileServices for Files {
    fn create_utf16(&mut self, _: u32, name: &[u16]) -> Result<Option<Box<dyn std::io::Write>>> {
        let Ok(name) = String::from_utf16(name) else {
            return Ok(None);
        };
        Ok(
            std::fs::File::create(std::path::Path::new(&self.root).join(name.replace('\\', "/")))
                .ok()
                .map(|file| Box::new(file) as Box<dyn std::io::Write>),
        )
    }

    fn create(&mut self, _: u32, name: &[u8]) -> Result<Option<Box<dyn std::io::Write>>> {
        let Some(name) =
            encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(name)
        else {
            return Ok(None);
        };
        Ok(
            std::fs::File::create(std::path::Path::new(&self.root).join(name.replace('\\', "/")))
                .ok()
                .map(|file| Box::new(file) as Box<dyn std::io::Write>),
        )
    }

    fn append_line(&mut self, _: u32, name: &[u8], text: &[u8]) -> Result<()> {
        use std::io::Write;

        let Some(name) =
            encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(name)
        else {
            return Ok(());
        };
        let name = name.replace('\\', "/");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::path::Path::new(&self.root).join(name))
        {
            let _ = file.write(text);
            let _ = file.write(b"\r\n");
        }
        Ok(())
    }
}

impl SaveServices for Saves {
    fn read(&mut self, _: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(std::fs::read(self.path(name)?).ok())
    }

    fn write(&mut self, _: u32, name: &[u8], bytes: &[u8]) -> Result<bool> {
        self.write_file(name, bytes, false)
    }

    fn write_preserving_time(&mut self, _: u32, name: &[u8], bytes: &[u8]) -> Result<bool> {
        self.write_file(name, bytes, true)
    }

    fn remove(&mut self, _: u32, name: &[u8]) -> Result<bool> {
        Ok(std::fs::remove_file(self.path(name)?).is_ok())
    }

    fn modified_time(&mut self, _: u32, name: &[u8]) -> Result<Option<u64>> {
        let Ok(metadata) = std::fs::metadata(self.path(name)?) else {
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
    use chrono::{Datelike, Timelike};
    let seconds = i64::try_from(seconds).map_err(|_| Error::new(0, "存档修改时间超过日期范围"))?;
    let date = chrono::DateTime::from_timestamp(seconds, 0)
        .ok_or_else(|| Error::new(0, "存档本地日期转换失败"))?
        .with_timezone(&chrono::Local);
    let values = [
        date.year(),
        date.month() as i32,
        date.day() as i32,
        date.hour() as i32,
        date.minute() as i32,
        date.second() as i32,
    ];
    let mut result = [0; 6];
    for (target, value) in result.iter_mut().zip(values) {
        *target = u32::try_from(value).map_err(|_| Error::new(0, "存档本地日期字段超出范围"))?;
    }
    Ok(result)
}

pub fn local_date_time() -> LocalDateTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    LocalDateTime {
        year: now.year() as u16,
        month: now.month() as u16,
        weekday: now.weekday().num_days_from_sunday() as u16,
        day: now.day() as u16,
        hour: now.hour() as u16,
        minute: now.minute() as u16,
        second: now.second() as u16,
    }
}

struct ProbeSystem;

impl SystemServices for ProbeSystem {
    fn local_date_time(&mut self) -> Option<LocalDateTime> {
        Some(local_date_time())
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
    if let Some(importer) = image_import::Import::from_startup(&root, &startup_xml)? {
        runtime.set_platform_job_services(importer);
    }
    runtime.host_mut().archive_config = Some(router.config.clone());
    runtime.host_mut().sound_groups = Some(katze_system2::audio::SoundGroups::from_startup_xml(
        startup.as_bytes(),
    )?);
    let configuration_files = ConfigurationFiles::new(&root, &startup_xml)?;
    runtime.host_mut().script_settings = configuration_files.read_script_settings()?;
    runtime.host_mut().folder_paths = Some(configuration_files.folders.clone());
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
    runtime.set_fonts(load_fonts(&root));
    runtime.set_file_services(Files { root });
    runtime.set_save_services(Saves {
        root: configuration_files.directory.to_string_lossy().into_owned(),
    });
    runtime.set_configuration_services(configuration_files);
    runtime.set_system_services(ProbeSystem);
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
