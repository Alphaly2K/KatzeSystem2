use crate::{
    Error, Result,
    io::{ReadSource, SharedSource, read_exact_from},
};
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, KeyInit},
};

const READ_CHUNK: usize = 32 * 1024;
const BLOCK_BATCH: usize = 128 * 1024;
const BULK_CHUNK: usize = 2 * 1024 * 1024;

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

    fn read_encrypted(
        &self,
        cipher: &BlowfishLE,
        offset: u64,
        output: &mut [u8],
        workers: usize,
    ) -> Result<()> {
        let start = offset & !7;
        let end = ((offset + output.len() as u64 + 7) & !7).min(self.length);
        let mut bytes = vec![0; (end - start) as usize];
        self.source.read_exact_at(self.offset + start, &mut bytes)?;
        if workers > 1 {
            let step = bytes.len().div_ceil(workers * 8) * 8;
            std::thread::scope(|scope| {
                let mut parts = bytes.chunks_mut(step);
                let first = parts.next().unwrap();
                for part in parts {
                    scope.spawn(move || decrypt_blocks(cipher, part));
                }
                decrypt_blocks(cipher, first);
            });
        } else {
            decrypt_blocks(cipher, &mut bytes);
        }
        let prefix = (offset - start) as usize;
        output.copy_from_slice(&bytes[prefix..prefix + output.len()]);
        Ok(())
    }
}

fn decrypt_blocks(cipher: &BlowfishLE, bytes: &mut [u8]) {
    for block in bytes.chunks_exact_mut(8) {
        cipher.decrypt_block(block.into());
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
        let count = count.min(READ_CHUNK);
        self.read_encrypted(cipher, offset, &mut output[..count], 1)?;
        Ok(count)
    }

    fn read_exact_at(&self, mut offset: u64, output: &mut [u8]) -> Result<()> {
        let Some(cipher) = &self.cipher else {
            return read_exact_from(self, offset, output);
        };
        if output.len() < 2 * BLOCK_BATCH
            || output.len() as u64 > self.length.saturating_sub(offset)
        {
            return read_exact_from(self, offset, output);
        }
        let max_workers =
            std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
        for chunk in output.chunks_mut(BULK_CHUNK) {
            let workers = (chunk.len() / BLOCK_BATCH).clamp(1, max_workers);
            if self.read_encrypted(cipher, offset, chunk, workers).is_err() {
                // 分块回退保留已成功解码的前缀，以及实际失败分块的诊断。
                read_exact_from(self, offset, chunk)?;
            }
            offset += chunk.len() as u64;
        }
        Ok(())
    }
}
