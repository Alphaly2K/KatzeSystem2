//! CSS 文件与带编号的数据记录；文件位置和读取由外层存档服务提供。

use crate::{Error, Result, read};
use std::io::Write;

const BLOCK: usize = 0x80000;

/// 按任务的存档目录读取 CSS 文件及查询文件修改时间，名称使用 CP932 字节。
/// `None` 表示文件不存在或无法打开；未接入所需服务时返回诊断。
pub trait SaveServices {
    fn read(&mut self, task_id: u32, name: &[u8]) -> Result<Option<Vec<u8>>>;

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

/// 连续的编号／长度／正文记录，保留重复编号、声明用量和逻辑容量。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataContainer {
    bytes: Vec<u8>,
    capacity: usize,
    used: u32,
    tag: u32,
}

impl DataContainer {
    pub fn new(tag: u32) -> Self {
        Self {
            bytes: vec![0; BLOCK],
            capacity: BLOCK,
            used: 0,
            tag,
        }
    }

    /// 非 CSS 签名返回 None；截断头部／压缩区或过大容量返回诊断。
    /// 解压错误、尾随输入和声明用量差异不使正确签名的文件加载失败。
    pub fn from_css(file: &[u8]) -> Result<Option<Self>> {
        if file.len() > read::MAX_BYTES {
            return Err(Error::new(0, "CSS 文件超过读取上限"));
        }
        if !file.starts_with(b"CSS\0") {
            return Ok(None);
        }
        let compressed = read::u32_at(file, 4)? as usize;
        let used = read::u32_at(file, 8)?;
        let tag = read::u32_at(file, 12)?;
        if used as usize >= read::MAX_BYTES {
            return Err(Error::new(8, "CSS 容量超过读取上限"));
        }
        let capacity = (used as usize / BLOCK + 1) * BLOCK;
        if capacity > read::MAX_BYTES {
            return Err(Error::new(8, "CSS 容量超过读取上限"));
        }
        let source = read::range(file, 16, compressed)?;
        let mut bytes = vec![0; capacity];
        // 原读取器忽略解压返回值，保留错误前已写出的数据和文件声明用量。
        let _ = flate2::Decompress::new(true).decompress(
            source,
            &mut bytes,
            flate2::FlushDecompress::Finish,
        );
        Ok(Some(Self {
            bytes,
            capacity,
            used,
            tag,
        }))
    }

    pub fn tag(&self) -> u32 {
        self.tag
    }

    /// 按逻辑容量写出 CSS 文件：四个头部 DWORD 后接 zlib 流。
    /// 未使用容量以零写出，因此声明用量之外已经解压出的记录仍可再次加载。
    pub fn to_css(&self) -> Result<Vec<u8>> {
        if self.used as usize > self.capacity || self.bytes.len() < self.capacity {
            return Err(Error::new(0, "CSS 声明用量超过逻辑容量"));
        }
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&self.bytes[..self.capacity])
            .map_err(|error| Error::new(0, format!("Zlib 压缩失败：{error}")))?;
        let compressed = encoder
            .finish()
            .map_err(|error| Error::new(0, format!("Zlib 压缩失败：{error}")))?;
        let length =
            u32::try_from(compressed.len()).map_err(|_| Error::new(0, "CSS 压缩区超过长度上限"))?;
        if compressed.len() > read::MAX_BYTES - 16 {
            return Err(Error::new(0, "CSS 文件超过读取上限"));
        }
        let mut file = Vec::with_capacity(16 + compressed.len());
        file.extend_from_slice(b"CSS\0");
        for value in [length, self.used, self.tag] {
            file.extend_from_slice(&value.to_le_bytes());
        }
        file.extend_from_slice(&compressed);
        Ok(file)
    }

    pub fn used_len(&self) -> u32 {
        self.used
    }

    /// 删除记录会缩小逻辑容量，底层分配保留至扩容或清空。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn used_bytes(&self) -> Result<&[u8]> {
        read::range(&self.bytes, 0, self.used as usize)
    }

    /// 嵌套记录的未压缩快照；首 DWORD 的原生地址不参与恢复，固定写零。
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let data = self.used_bytes()?;
        if data.len() > read::MAX_BYTES - 16 {
            return Err(Error::new(0, "CSS 快照超过读取上限"));
        }
        let mut bytes = Vec::with_capacity(16 + data.len());
        for value in [0, self.capacity as u32, self.used, self.tag] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(data);
        Ok(bytes)
    }

    /// 从嵌套记录恢复独立容器，保留快照中的逻辑容量、用量和标签。
    pub fn from_snapshot(snapshot: &[u8]) -> Result<Self> {
        let capacity = read::u32_at(snapshot, 4)? as usize;
        let used = read::u32_at(snapshot, 8)?;
        let tag = read::u32_at(snapshot, 12)?;
        if capacity > read::MAX_BYTES || used as usize > capacity {
            return Err(Error::new(4, "CSS 快照容量或用量越界"));
        }
        let data = read::range(snapshot, 16, used as usize)?;
        let mut bytes = vec![0; capacity];
        bytes[..data.len()].copy_from_slice(data);
        Ok(Self {
            bytes,
            capacity,
            used,
            tag,
        })
    }

    /// 读取首个同编号记录；查找扫描逻辑容量，包含声明用量之后的解压数据。
    pub fn record(&self, id: u32) -> Result<Option<&[u8]>> {
        let Some((start, length)) = self.find(id)? else {
            return Ok(None);
        };
        Ok(Some(read::range(&self.bytes, start + 8, length)?))
    }

    pub fn append(&mut self, id: u32, data: &[u8]) -> Result<()> {
        let mut header = [0; 8];
        header[..4].copy_from_slice(&id.to_le_bytes());
        let length =
            u32::try_from(data.len()).map_err(|_| Error::new(0, "CSS 记录超过长度上限"))?;
        header[4..].copy_from_slice(&length.to_le_bytes());
        self.append_bytes(&header)?;
        self.append_bytes(data)
    }

    /// 删除首个同编号记录。记录末端等于容量时保持原容器。
    pub fn remove(&mut self, id: u32) -> Result<bool> {
        let Some((start, length)) = self.find(id)? else {
            return Ok(false);
        };
        let size = length
            .checked_add(8)
            .ok_or_else(|| Error::new(start, "CSS 记录长度溢出"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::new(start, "CSS 记录范围溢出"))?;
        if end >= self.capacity {
            return Ok(false);
        }
        self.bytes.copy_within(end..self.capacity, start);
        self.capacity -= size;
        self.used = self.used.wrapping_sub(size as u32);
        Ok(true)
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.tag);
    }

    fn find(&self, id: u32) -> Result<Option<(usize, usize)>> {
        let mut start = 0;
        while start < self.capacity {
            let Some(header) = self.bytes.get(start..start + 8) else {
                return Ok(None);
            };
            let length = read::u32_at(header, 4)? as usize;
            if read::u32_at(header, 0)? == id {
                return Ok(Some((start, length)));
            }
            start = start
                .checked_add(8)
                .and_then(|n| n.checked_add(length))
                .ok_or_else(|| Error::new(start, "CSS 记录范围溢出"))?;
        }
        Ok(None)
    }

    fn append_bytes(&mut self, data: &[u8]) -> Result<()> {
        let start = self.used as usize;
        let end = start
            .checked_add(data.len())
            .filter(|&end| end <= read::MAX_BYTES)
            .ok_or_else(|| Error::new(start, "CSS 记录超过读取上限"))?;
        let blocks = end / BLOCK - start / BLOCK;
        if blocks != 0 {
            let capacity = self.capacity + blocks * BLOCK;
            if capacity > read::MAX_BYTES {
                return Err(Error::new(start, "CSS 容量超过读取上限"));
            }
            let mut bytes = vec![0; capacity];
            bytes[..self.capacity].copy_from_slice(&self.bytes[..self.capacity]);
            self.bytes = bytes;
            self.capacity = capacity;
        }
        let target = self
            .bytes
            .get_mut(start..end)
            .ok_or_else(|| Error::new(start, "CSS 写入超出分配范围"))?;
        target.copy_from_slice(data);
        self.used = end as u32;
        Ok(())
    }
}
