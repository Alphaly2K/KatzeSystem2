use katze_system2::{
    Error, Result,
    host::storage::{FileServices, SaveServices},
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Saves {
    pub(super) root: PathBuf,
}

impl Saves {
    fn path(&self, name: &[u8]) -> Result<PathBuf> {
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(name)
            .ok_or_else(|| Error::new(0, "存档名称包含无效 CP932"))?
            .replace('\\', "/");
        Ok(self.root.join(name))
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

pub(super) struct Files {
    pub(super) root: PathBuf,
}

impl FileServices for Files {
    fn create_utf16(&mut self, _: u32, name: &[u16]) -> Result<Option<Box<dyn std::io::Write>>> {
        let Ok(name) = String::from_utf16(name) else {
            return Ok(None);
        };
        Ok(
            std::fs::File::create(self.root.join(name.replace('\\', "/")))
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
            std::fs::File::create(self.root.join(name.replace('\\', "/")))
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
            .open(self.root.join(name))
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
        Ok(file_time(modified))
    }

    fn modified_date(&mut self, owner: u32, name: &[u8]) -> Result<Option<[u32; 6]>> {
        self.modified_time(owner, name)?
            .map(local_save_date)
            .transpose()
    }
}

const UNIX_EPOCH_FILE_TIME: i128 = 116_444_736_000_000_000;

fn file_time(time: SystemTime) -> Option<u64> {
    let nanos = match time.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_nanos() as i128,
        Err(before_epoch) => -(before_epoch.duration().as_nanos() as i128),
    };
    u64::try_from(UNIX_EPOCH_FILE_TIME + nanos.div_euclid(100)).ok()
}

fn unix_seconds(file_time: u64) -> i128 {
    (i128::from(file_time) - UNIX_EPOCH_FILE_TIME).div_euclid(10_000_000)
}

fn local_save_date(file_time: u64) -> Result<[u32; 6]> {
    let seconds = unix_seconds(file_time);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn save_file_times_keep_subsecond_order_on_both_sides_of_1970() {
        let delta = Duration::from_nanos(100);
        let before = file_time(UNIX_EPOCH - delta).unwrap();
        let epoch = file_time(UNIX_EPOCH).unwrap();
        let after = file_time(UNIX_EPOCH + delta).unwrap();
        assert_eq!(epoch - before, 1);
        assert_eq!(after - epoch, 1);
        assert_eq!(unix_seconds(before), -1);
        assert_eq!(unix_seconds(epoch), 0);
        assert_eq!(unix_seconds(after), 0);
    }
}
