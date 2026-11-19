use katze_system2::{Error, Result, host::ReadSource};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
};

pub(super) struct FileSource {
    file: Mutex<BufferedFile>,
}

const FILE_BUFFER_SIZE: usize = 256 * 1024;

struct BufferedFile {
    reader: BufReader<File>,
    position: Option<u64>,
}

impl FileSource {
    pub(super) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file =
            File::open(path).map_err(|error| Error::new(0, format!("文件打开失败：{error}")))?;
        Self::from_file(file)
    }

    pub(super) fn from_file(file: File) -> Result<Self> {
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
