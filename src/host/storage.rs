use crate::{Error, Result};
use std::{io::Write, sync::Arc};

pub type SharedSource = Arc<dyn ReadSource>;

/// Identifies an entry in an immutable resource snapshot. Use a new generation
/// when replacing content or remounting a source.
#[derive(Clone)]
pub struct ReadSnapshot {
    generation: Arc<()>,
    entry: usize,
}

impl ReadSnapshot {
    pub fn new(generation: Arc<()>, entry: usize) -> Self {
        Self { generation, entry }
    }
}

impl PartialEq for ReadSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.entry == other.entry && Arc::ptr_eq(&self.generation, &other.generation)
    }
}

impl Eq for ReadSnapshot {}

/// Random access without a shared cursor. Calls may come from decoder workers.
pub trait ReadSource: Send + Sync {
    fn len(&self) -> Result<u64>;

    /// Return an identity only if the host keeps this snapshot immutable.
    fn snapshot(&self) -> Option<ReadSnapshot> {
        None
    }

    fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Short reads are allowed. Return zero at EOF and never report more bytes
    /// than fit in `output`.
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize>;

    fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> Result<()> {
        crate::io::read_exact_from(self, offset, output)
    }
}

/// Script file output. Names use CP932 unless the method accepts UTF-16.
/// The host resolves names independently of resources and saves.
pub trait FileServices {
    /// Create or truncate a file by wide name. The default uses the CP932 service.
    fn create_utf16(&mut self, task_id: u32, name: &[u16]) -> Result<Option<Box<dyn Write>>> {
        let name = &name[..name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())];
        let name =
            String::from_utf16(name).map_err(|_| Error::new(0, "文件名称包含无效 UTF-16"))?;
        let name = crate::text::cp932_encoded(&name)
            .map_err(|_| Error::new(0, "文件输出服务需要宽字符名称支持"))?;
        self.create(task_id, &name)
    }

    /// Create or truncate a file. Return `None` for an ordinary open failure.
    fn create(&mut self, _task_id: u32, _name: &[u8]) -> Result<Option<Box<dyn Write>>> {
        Err(Error::new(0, "宿主未提供文件创建服务"))
    }

    /// Attempt to append the text, then CRLF. The host may create a missing file.
    /// Ordinary open or write failures are silent; use `Err` for integration errors.
    fn append_line(&mut self, task_id: u32, name: &[u8], text: &[u8]) -> Result<()>;
}

/// 按任务的存档目录读写 CSS 文件及查询文件修改时间，名称使用 CP932 字节。
/// `None` 表示文件不存在或无法打开；未接入所需服务时返回诊断。
pub trait SaveServices {
    fn read(&mut self, task_id: u32, name: &[u8]) -> Result<Option<Vec<u8>>>;

    /// 写入完整 CSS 文件，覆盖已有内容；无法打开或完成写入时返回 false，短写后删除文件。
    fn write(&mut self, _task_id: u32, _name: &[u8], _bytes: &[u8]) -> Result<bool> {
        Err(Error::new(0, "存档写入服务尚未接入"))
    }

    /// 覆盖 CSS 文件并尝试保留已有文件的修改时间；文件不存在时正常创建。
    /// 无法打开或完成写入时返回 false，短写后删除不完整文件；恢复时间失败不改变写入结果。
    fn write_preserving_time(
        &mut self,
        _task_id: u32,
        _name: &[u8],
        _bytes: &[u8],
    ) -> Result<bool> {
        Err(Error::new(0, "存档保留时间写入服务尚未接入"))
    }

    /// 删除同一目录下的文件；文件不存在或删除失败时返回 false。
    fn remove(&mut self, _task_id: u32, _name: &[u8]) -> Result<bool> {
        Err(Error::new(0, "存档删除服务尚未接入"))
    }

    /// 查询同一目录下文件的最后修改时间，使用自 1601-01-01 UTC 起的 100 ns 计数。
    /// None 表示文件无法打开；未接入元数据查询时返回诊断。
    fn modified_time(&mut self, _task_id: u32, _name: &[u8]) -> Result<Option<u64>> {
        Err(Error::new(0, "存档修改时间查询服务尚未接入"))
    }

    /// 查询文件修改时间的本地年、月、日、时、分、秒；None 表示无法查询。
    fn modified_date(&mut self, _task_id: u32, _name: &[u8]) -> Result<Option<[u32; 6]>> {
        Err(Error::new(0, "存档本地日期查询服务尚未接入"))
    }
}

/// 用户配置的两个独立文件，具体路径由平台宿主决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationFile {
    Main,
    Sound,
}

/// 同步配置文件写入；普通打开或写入失败返回 false，集成诊断使用 Err。
pub trait ConfigurationServices {
    fn write(&mut self, task_id: u32, file: ConfigurationFile, bytes: &[u8]) -> Result<bool>;

    /// Persist the snapshot used by script setting natives. The default keeps it in memory.
    fn write_script_settings(&mut self, _task_id: u32, _bytes: &[u8]) -> Result<bool> {
        Ok(true)
    }
}
