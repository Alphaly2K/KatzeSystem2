use super::{KifArchive, terminated};
use crate::{Error, Result, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ResourceMode {
    File = 1,
    Archive = 2,
    LooseFirst = 3,
    ArchiveFirst = 4,
}

/// path 为归档装载时保存的路径；可与所在槽位的 prefix 不同。
pub struct ArchiveMount<'a> {
    pub path: Vec<u16>,
    pub archive: &'a KifArchive<'a>,
}

#[derive(Default)]
pub struct ResourceSlot<'a> {
    pub prefix: Vec<u16>,
    pub directory: Vec<u16>,
    pub loose_enabled: bool,
    pub archive: Option<ArchiveMount<'a>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceMember {
    pub slot: Option<u8>,
    pub directory: Vec<u16>,
    pub loose_enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceGroup {
    pub prefix: Vec<u16>,
    pub members: Vec<ResourceMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceSource {
    File(Vec<u16>),
    Archive(u8),
    Patch(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceData {
    pub source: ResourceSource,
    pub bytes: Vec<u8>,
}

/// 显式注册归档、列表成员和补丁；文件读取由宿主提供。
pub struct ResourceRouter<'a> {
    pub slots: [ResourceSlot<'a>; 256],
    pub groups: Vec<ResourceGroup>,
    pub patches: Vec<&'a KifArchive<'a>>,
}

impl Default for ResourceRouter<'_> {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| ResourceSlot::default()),
            groups: Vec::new(),
            patches: Vec::new(),
        }
    }
}

impl ResourceRouter<'_> {
    /// 输入为 CP932 虚拟名称。回调返回 None 表示打开失败并继续回退。
    pub fn read(
        &self,
        name: &[u8],
        mode: ResourceMode,
        file: impl FnMut(&[u16]) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<ResourceData>> {
        let name: Vec<u16> = text::cp932(terminated(name))?.encode_utf16().collect();
        self.read_utf16(&name, mode, file)
    }

    /// 宽字符入口；前缀目前限定 ASCII，内部名称及散装目录保留 UTF-16。
    pub fn read_utf16(
        &self,
        name: &[u16],
        mode: ResourceMode,
        mut file: impl FnMut(&[u16]) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<ResourceData>> {
        let name = wide_terminated(name);
        check_length(name)?;
        let (prefix, basename) = match name.iter().position(|&c| c == b'/' as u16) {
            Some(i) => (&name[..i], &name[i + 1..]),
            None => (&name[..0], name),
        };
        for (i, archive) in self.patches.iter().enumerate() {
            if let Some(bytes) = read_archive(archive, basename)? {
                return Ok(Some(ResourceData {
                    source: ResourceSource::Patch(i),
                    bytes,
                }));
            }
        }
        if mode == ResourceMode::File {
            return read_file(name, &mut file);
        }
        if eq_prefix(extension(prefix), &[b'l' as u16, b's' as u16, b't' as u16])? {
            for group in &self.groups {
                if !eq_prefix(prefix, &group.prefix)? {
                    continue;
                }
                for member in &group.members {
                    let archive = member.slot.and_then(|i| {
                        self.slots[i as usize]
                            .archive
                            .as_ref()
                            .map(|a| (i, a.archive))
                    });
                    if let Some(data) = read_pair(
                        archive,
                        member.loose_enabled.then_some(member.directory.as_slice()),
                        basename,
                        mode,
                        true,
                        &mut file,
                    )? {
                        return Ok(Some(data));
                    }
                }
            }
            return Ok(None);
        }
        for (i, slot) in self.slots.iter().enumerate() {
            if mode == ResourceMode::Archive {
                if let Some(mount) = &slot.archive {
                    if eq_prefix(prefix, &mount.path)? {
                        if let Some(bytes) = read_archive(mount.archive, basename)? {
                            return Ok(Some(ResourceData {
                                source: ResourceSource::Archive(i as u8),
                                bytes,
                            }));
                        }
                    }
                }
                continue;
            }
            // LooseFirst 的空前缀遍历全部槽位，且强制尝试每个散装目录。
            let unqualified = mode == ResourceMode::LooseFirst && prefix.is_empty();
            if !unqualified && !eq_prefix(prefix, &slot.prefix)? {
                continue;
            }
            if let Some(data) = read_pair(
                slot.archive.as_ref().map(|a| (i as u8, a.archive)),
                (unqualified || slot.loose_enabled).then_some(slot.directory.as_slice()),
                basename,
                mode,
                mode == ResourceMode::ArchiveFirst,
                &mut file,
            )? {
                return Ok(Some(data));
            }
        }
        Ok(None)
    }
}

fn read_pair(
    archive: Option<(u8, &KifArchive<'_>)>,
    loose: Option<&[u16]>,
    basename: &[u16],
    mode: ResourceMode,
    always_separator: bool,
    file: &mut impl FnMut(&[u16]) -> Result<Option<Vec<u8>>>,
) -> Result<Option<ResourceData>> {
    let read_mounted = || -> Result<Option<ResourceData>> {
        match archive {
            Some((slot, archive)) => {
                Ok(read_archive(archive, basename)?.map(|bytes| ResourceData {
                    source: ResourceSource::Archive(slot),
                    bytes,
                }))
            }
            None => Ok(None),
        }
    };
    if mode != ResourceMode::LooseFirst {
        if let Some(data) = read_mounted()? {
            return Ok(Some(data));
        }
    }
    if let Some(directory) = loose.filter(|_| mode != ResourceMode::Archive) {
        let directory = wide_terminated(directory);
        let mut path = directory.to_vec();
        if always_separator || !directory.is_empty() {
            path.push(b'\\' as u16);
        }
        path.extend(basename);
        check_length(&path)?;
        if let Some(data) = read_file(&path, file)? {
            return Ok(Some(data));
        }
    }
    if mode == ResourceMode::LooseFirst {
        return read_mounted();
    }
    Ok(None)
}

fn read_archive(archive: &KifArchive<'_>, name: &[u16]) -> Result<Option<Vec<u8>>> {
    archive
        .directory()
        .find_utf16(name)
        .map(|entry| archive.read_entry(entry))
        .transpose()
}

fn read_file(
    name: &[u16],
    file: &mut impl FnMut(&[u16]) -> Result<Option<Vec<u8>>>,
) -> Result<Option<ResourceData>> {
    Ok(file(name)?.map(|bytes| ResourceData {
        source: ResourceSource::File(name.to_vec()),
        bytes,
    }))
}

fn wide_terminated(name: &[u16]) -> &[u16] {
    &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]
}

fn eq_prefix(left: &[u16], right: &[u16]) -> Result<bool> {
    let left = wide_terminated(left);
    let right = wide_terminated(right);
    if left.iter().chain(right).any(|&c| c > 127) {
        return Err(Error::new(0, "资源路由前缀暂只支持 ASCII"));
    }
    Ok(left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(&a, &b)| (a as u8).eq_ignore_ascii_case(&(b as u8))))
}

fn extension(name: &[u16]) -> &[u16] {
    for (i, &c) in name.iter().enumerate().rev() {
        if c == b'/' as u16 || c == b'\\' as u16 {
            break;
        }
        if c == b'.' as u16 {
            return &name[i + 1..];
        }
    }
    &name[name.len()..]
}

fn check_length(name: &[u16]) -> Result<()> {
    if name.len() >= 260 {
        return Err(Error::new(0, "资源路由路径超过基线容量"));
    }
    Ok(())
}
