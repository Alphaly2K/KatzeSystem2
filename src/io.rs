//! 随机读取源与脚本文件写入服务；目录及资源路由由宿主提供。

use crate::{Error, Result};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
};

pub type SharedSource = Arc<dyn ReadSource>;

/// 脚本文件写入；名称为 CP932 字节，目录策略独立于资源与存档路由。
pub trait FileServices {
    /// 创建或截断宽字符名称指向的文件；默认桥接 CP932 创建服务。
    fn create_utf16(
        &mut self,
        task_id: u32,
        name: &[u16],
    ) -> Result<Option<Box<dyn std::io::Write>>> {
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

    /// 创建或截断文件并打开顺序写入端；普通打开失败返回 None。
    fn create(&mut self, _task_id: u32, _name: &[u8]) -> Result<Option<Box<dyn std::io::Write>>> {
        Err(Error::new(0, "宿主未提供文件创建服务"))
    }

    /// 在文件末尾依次尝试写入正文及 CRLF，缺失文件可创建。
    /// 普通打开／写入失败仍正常结束；服务不可用或协议错误返回诊断。
    fn append_line(&mut self, task_id: u32, name: &[u8], text: &[u8]) -> Result<()>;
}

/// 读取不改变共享游标；多个读取者可独立访问同一来源。
pub trait ReadSource: Send + Sync {
    fn len(&self) -> Result<u64>;

    fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// 允许短读；到达末尾时返回零，返回量不得超过输出缓冲区。
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize>;

    fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> Result<()> {
        read_exact_from(self, offset, output)
    }
}

pub(crate) fn read_exact_from<S: ReadSource + ?Sized>(
    source: &S,
    mut offset: u64,
    mut output: &mut [u8],
) -> Result<()> {
    while !output.is_empty() {
        let position = usize::try_from(offset).unwrap_or(usize::MAX);
        let count = source.read_at(offset, output)?;
        if count == 0 {
            return Err(Error::new(position, "读取源提前结束"));
        }
        if count > output.len() {
            return Err(Error::new(position, "读取源返回量超过输出缓冲区"));
        }
        offset = offset
            .checked_add(count as u64)
            .ok_or_else(|| Error::new(position, "读取源位置溢出"))?;
        output = &mut output[count..];
    }
    Ok(())
}

impl ReadSource for [u8] {
    fn len(&self) -> Result<u64> {
        Ok(<[u8]>::len(self) as u64)
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        let Some(bytes) = usize::try_from(offset)
            .ok()
            .and_then(|offset| self.get(offset..))
        else {
            return Ok(0);
        };
        let count = bytes.len().min(output.len());
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

impl ReadSource for Vec<u8> {
    fn len(&self) -> Result<u64> {
        ReadSource::len(self.as_slice())
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        self.as_slice().read_at(offset, output)
    }
}

/// 独占打开的文件句柄；文件游标由随机读取操作内部协调。
pub struct FileSource {
    file: Mutex<BufferedFile>,
}

const FILE_BUFFER_SIZE: usize = 256 * 1024;

struct BufferedFile {
    reader: BufReader<File>,
    position: Option<u64>,
}

impl FileSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file =
            File::open(path).map_err(|error| Error::new(0, format!("文件打开失败：{error}")))?;
        if !file
            .metadata()
            .map_err(|error| Error::new(0, format!("文件属性读取失败：{error}")))?
            .is_file()
        {
            return Err(Error::new(0, "文件读取源需要普通文件"));
        }
        Ok(Self {
            file: Mutex::new(BufferedFile {
                reader: BufReader::with_capacity(FILE_BUFFER_SIZE, file),
                position: Some(0),
            }),
        })
    }
}

impl ReadSource for FileSource {
    fn len(&self) -> Result<u64> {
        self.file
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reader
            .get_ref()
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| Error::new(0, format!("文件长度查询失败：{error}")))
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let mut file = self.file.lock().unwrap_or_else(|error| error.into_inner());
        let error = |error| {
            Error::new(
                usize::try_from(offset).unwrap_or(usize::MAX),
                format!("文件读取失败：{error}"),
            )
        };
        let position = file.position.take();
        if let Some(position) =
            position.filter(|&position| offset.abs_diff(position) < FILE_BUFFER_SIZE as u64)
        {
            if offset != position {
                let distance = (i128::from(offset) - i128::from(position)) as i64;
                file.reader.seek_relative(distance).map_err(error)?;
            }
        } else {
            file.reader.seek(SeekFrom::Start(offset)).map_err(error)?;
        }
        let count = file.reader.read(output).map_err(error)?;
        file.position = offset.checked_add(count as u64);
        Ok(count)
    }
}

/// 为每个解码器保存独立位置，将随机读取源适配为标准 Read／Seek。
pub struct SourceReader<'a, S: ReadSource + ?Sized> {
    source: ReaderSource<'a, S>,
    position: u64,
}

enum ReaderSource<'a, S: ReadSource + ?Sized> {
    Borrowed(&'a S),
    Shared(SharedSource),
}

impl<S: ReadSource + ?Sized> ReadSource for ReaderSource<'_, S> {
    fn len(&self) -> Result<u64> {
        match self {
            Self::Borrowed(source) => source.len(),
            Self::Shared(source) => source.len(),
        }
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        match self {
            Self::Borrowed(source) => source.read_at(offset, output),
            Self::Shared(source) => source.read_at(offset, output),
        }
    }
}

impl<'a, S: ReadSource + ?Sized> SourceReader<'a, S> {
    pub fn new(source: &'a S) -> Self {
        Self {
            source: ReaderSource::Borrowed(source),
            position: 0,
        }
    }

    pub(crate) fn reopen(&self) -> Self {
        Self {
            source: match &self.source {
                ReaderSource::Borrowed(source) => ReaderSource::Borrowed(source),
                ReaderSource::Shared(source) => ReaderSource::Shared(source.clone()),
            },
            position: 0,
        }
    }
}

impl SourceReader<'static, dyn ReadSource> {
    /// 持有共享来源，可随解码器移动到其他线程；读取位置仍由本游标独立保存。
    pub fn from_shared(source: SharedSource) -> Self {
        Self {
            source: ReaderSource::Shared(source),
            position: 0,
        }
    }
}

impl<S: ReadSource + ?Sized> Read for SourceReader<'_, S> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let count = self
            .source
            .read_at(self.position, output)
            .map_err(std::io::Error::other)?;
        if count > output.len() {
            return Err(std::io::Error::other("读取源返回量超过输出缓冲区"));
        }
        self.position = self
            .position
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("读取源位置溢出"))?;
        Ok(count)
    }
}

impl<S: ReadSource + ?Sized> Seek for SourceReader<'_, S> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let position = match position {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => {
                i128::from(self.source.len().map_err(std::io::Error::other)?) + i128::from(offset)
            }
        };
        self.position = u64::try_from(position).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "读取位置超出范围")
        })?;
        Ok(self.position)
    }
}
