use super::{HostState, Interpreter, NativeReturn, native, strings};
use crate::{Error, Result, read, text};
use std::collections::BTreeMap;

const MAGIC: &[u8; 8] = b"KCSSET01";

/// Host-owned settings shared by scripts within one runtime.
#[derive(Default)]
pub struct ScriptSettings {
    groups: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    record_bytes: usize,
}

impl ScriptSettings {
    /// Maximum encoded snapshot size in bytes.
    pub const MAX_SNAPSHOT_BYTES: usize = read::MAX_BYTES;

    /// Load a snapshot produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > Self::MAX_SNAPSHOT_BYTES || !bytes.starts_with(MAGIC) {
            return Err(Error::new(0, "无效的脚本设置文件"));
        }
        let mut settings = Self::default();
        let mut offset = MAGIC.len();
        while offset != bytes.len() {
            let mut field = || {
                let size = read::sized(read::u32_at(bytes, offset)?, offset)?;
                offset += 4;
                let field = read::range(bytes, offset, size)?;
                offset += size;
                Ok::<_, Error>(field)
            };
            let group = decode_key(field()?)?;
            let name = decode_key(field()?)?;
            let data = field()?;
            settings.record_bytes = settings.next_size(&group, &name, data.len())?;
            if settings
                .groups
                .entry(group)
                .or_default()
                .insert(name, data.to_vec())
                .is_some()
            {
                return Err(Error::new(offset, "脚本设置键重复"));
            }
        }
        Ok(settings)
    }

    /// Encode the complete snapshot, keeping setting values as raw bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(MAGIC.len() + self.record_bytes);
        result.extend(MAGIC);
        for (group, entries) in &self.groups {
            for (name, bytes) in entries {
                for field in [group.as_bytes(), name.as_bytes(), bytes.as_slice()] {
                    result.extend((field.len() as u32).to_le_bytes());
                    result.extend(field);
                }
            }
        }
        result
    }

    fn next_size(&self, group: &str, name: &str, length: usize) -> Result<usize> {
        let header = 12 + group.len() + name.len();
        let old = self.groups.get(group).and_then(|entries| entries.get(name));
        let size =
            self.record_bytes - old.map_or(0, |bytes| header + bytes.len()) + header + length;
        if size > Self::MAX_SNAPSHOT_BYTES - MAGIC.len() {
            return Err(Error::new(0, "脚本设置文件超过读取上限"));
        }
        Ok(size)
    }
}

fn fold_key(key: &str) -> String {
    key.chars().flat_map(char::to_uppercase).collect()
}

fn decode_key(bytes: &[u8]) -> Result<String> {
    let key = std::str::from_utf8(bytes).map_err(|_| Error::new(0, "脚本设置键不是 UTF-8"))?;
    Ok(fold_key(key))
}

fn key(vm: &Interpreter, address: u32) -> Result<String> {
    let source = strings::format(vm, address, 0)?;
    let source = source.split(|&byte| byte == 0).next().unwrap_or_default();
    let key = text::cp932_replacing(source);
    if key.encode_utf16().count() >= 256 {
        return Err(Error::new(vm.pc, "脚本设置键超过缓冲区上限"));
    }
    Ok(fold_key(&key))
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let args = native::words(arguments, [2, 4, 3, 3][(index - 192) as usize], vm.pc)?;
    let group = key(vm, args[0])?;
    if index <= 193 {
        let Some(entries) = host.script_settings.groups.get(&group) else {
            return Ok(NativeReturn::Value(0));
        };
        let name = key(vm, args[1])?;
        let Some(bytes) = entries.get(&name) else {
            return Ok(NativeReturn::Value(0));
        };
        if index == 192 {
            return Ok(NativeReturn::Value(u32::from(bytes.len() <= 4)));
        }
        if vm.read(args[2], 1).is_ok() {
            if bytes.len() > args[3] as usize {
                return Ok(NativeReturn::Value(0));
            }
            vm.write(args[2], bytes)?;
        }
        return Ok(NativeReturn::Value(bytes.len() as u32));
    }
    let name = key(vm, args[1])?;
    let bytes = if index == 194 {
        args[2].to_le_bytes().to_vec()
    } else {
        vm.read_string(args[2])?.to_vec()
    };
    let previous_size = host.script_settings.record_bytes;
    host.script_settings.record_bytes =
        host.script_settings.next_size(&group, &name, bytes.len())?;
    let old = host
        .script_settings
        .groups
        .entry(group.clone())
        .or_default()
        .insert(name.clone(), bytes);
    let result = match &mut host.configuration_services {
        Some(services) => {
            services.write_script_settings(host.task_id, &host.script_settings.to_bytes())
        }
        None => Ok(true),
    };
    if !matches!(result, Ok(true)) {
        host.script_settings.record_bytes = previous_size;
        let entries = host.script_settings.groups.get_mut(&group).unwrap();
        if let Some(previous) = old {
            entries.insert(name, previous);
        } else {
            entries.remove(&name);
            if entries.is_empty() {
                host.script_settings.groups.remove(&group);
            }
        }
    }
    Ok(NativeReturn::Value(u32::from(result?)))
}
