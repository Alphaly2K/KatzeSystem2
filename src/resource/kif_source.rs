use crate::{
    Error, Result,
    io::{ReadSource, SharedSource},
};
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, KeyInit},
};

/// 归档条目的独立随机读取视图；完整八字节块解密，短尾保留原值。
pub struct KifEntrySource {
    source: SharedSource,
    offset: u64,
    length: u64,
    cipher: Option<BlowfishLE>,
}

impl KifEntrySource {
    /// key 为归档目录长度种子生成的原始 DWORD；未加密条目使用 None。
    pub fn new(source: SharedSource, offset: u64, length: u32, key: Option<u32>) -> Result<Self> {
        let length = u64::from(length);
        let source_length = source.len()?;
        if offset
            .checked_add(length)
            .is_none_or(|end| end > source_length)
        {
            return Err(Error::new(0, "KIF 条目超出读取源范围"));
        }
        let cipher = key.map(|key| BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap());
        Ok(Self {
            source,
            offset,
            length,
            cipher,
        })
    }
}

impl ReadSource for KifEntrySource {
    fn len(&self) -> Result<u64> {
        Ok(self.length)
    }

    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        if offset >= self.length || output.is_empty() {
            return Ok(0);
        }
        let count = (self.length - offset).min(output.len() as u64) as usize;
        let Some(cipher) = &self.cipher else {
            let read = self
                .source
                .read_at(self.offset + offset, &mut output[..count])?;
            if read > count {
                return Err(Error::new(0, "KIF 读取源返回量超过输出缓冲区"));
            }
            return Ok(read);
        };
        let count = count.min(32768);
        let start = offset & !7;
        let end = ((offset + count as u64 + 7) & !7).min(self.length);
        let mut bytes = vec![0; (end - start) as usize];
        self.source.read_exact_at(self.offset + start, &mut bytes)?;
        for block in bytes.chunks_exact_mut(8) {
            cipher.decrypt_block(block.into());
        }
        let prefix = (offset - start) as usize;
        output[..count].copy_from_slice(&bytes[prefix..prefix + count]);
        Ok(count)
    }
}
