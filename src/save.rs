//! CSS 文件与带编号的数据记录；文件位置和读取由外层存档服务提供。

use crate::{Error, Result, read};

const BLOCK: usize = 0x80000;

/// 接收已展开的 CP932 文件名，并按任务的存档目录读取原始 CSS 文件。
/// `None` 表示文件不存在或无法打开；未配置服务时宿主返回诊断。
pub trait SaveServices {
    fn read(&mut self, task_id: u32, name: &[u8]) -> Result<Option<Vec<u8>>>;
}

/// 连续的编号／长度／正文记录，保留重复编号、声明用量和逻辑容量。
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
