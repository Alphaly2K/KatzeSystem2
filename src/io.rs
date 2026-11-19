//! Stream adapters for host-provided sources.

pub use crate::host::{FileServices, ReadSnapshot, ReadSource, SharedSource};
use crate::{Error, Result};
use std::io::{Read, Seek, SeekFrom};

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
