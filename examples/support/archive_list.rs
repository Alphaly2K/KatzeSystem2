use katze_system2::{Error, Result};
use std::{fs::File, io::Read, path::Path};

pub(super) struct Member {
    pub directory: String,
    pub archive: String,
}

pub(super) fn is_list(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("lst"))
}

pub(super) fn load(path: &Path) -> Result<Vec<Member>> {
    const MAX_BYTES: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| Error::new(0, format!("归档清单读取失败：{error}")))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::new(0, "归档清单超过示例宿主容量"));
    }
    let contents = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
    if !contents.is_ascii() {
        return Err(Error::new(0, "归档清单需要 ASCII 路径"));
    }
    let contents = std::str::from_utf8(contents).unwrap();
    let mut members = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let invalid = || Error::new(index + 1, "归档清单成员需要散装目录和归档文件名");
        let (directory, archive) = line.split_once('/').ok_or_else(invalid)?;
        if archive.is_empty()
            || archive.contains(['/', '\\'])
            || is_list(archive)
            || !relative_path(archive)
            || (!directory.is_empty() && !relative_path(directory))
        {
            return Err(invalid());
        }
        members.push(Member {
            directory: directory.replace('\\', "/"),
            archive: archive.to_string(),
        });
    }
    Ok(members)
}

fn relative_path(path: &str) -> bool {
    !path.contains([':', '\r'])
        && !path.starts_with(['/', '\\'])
        && !path.split(['/', '\\']).any(|part| part == "..")
        && path != "."
}
