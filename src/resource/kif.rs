use super::{ArchiveDirectory, DirectoryEntry, directory, terminated};
use crate::{Result, kcs::ScriptRandom, read};
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, KeyInit},
};

/// 从已解密的校验码计算归档名称种子；首个 NUL 终止校验码。
pub fn verification_seed(code: &[u8]) -> u32 {
    let mut seed = u32::MAX;
    for &byte in terminated(code) {
        seed ^= (byte as u32) << 24;
        for _ in 0..8 {
            seed = (seed << 1)
                ^ if seed & 0x8000_0000 != 0 {
                    0x04c1_1db7
                } else {
                    0
                };
        }
        seed = !seed;
    }
    seed
}

/// 借用完整 KIF 输入，索引解密后的目录，按需返回条目明文。
pub struct KifArchive<'a> {
    bytes: &'a [u8],
    directory: ArchiveDirectory,
    cipher: Option<BlowfishLE>,
}

impl<'a> KifArchive<'a> {
    /// name_seed 来自 verification_seed，未加密归档忽略该值。
    pub fn parse(bytes: &'a [u8], name_seed: u32) -> Result<Self> {
        let mut entries = directory::raw_entries(bytes)?;
        let cipher = if directory::encrypted(&entries) {
            let key = ScriptRandom::seeded(entries[0].length).next_integer();
            let cipher = BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap();
            for (i, entry) in entries.iter_mut().enumerate().skip(1) {
                let mut pair = [0; 8];
                pair[..4].copy_from_slice(&entry.offset.wrapping_add(i as u32).to_le_bytes());
                pair[4..].copy_from_slice(&entry.length.to_le_bytes());
                cipher.decrypt_block((&mut pair).into());
                entry.offset = u32::from_le_bytes(pair[..4].try_into().unwrap());
                entry.length = u32::from_le_bytes(pair[4..].try_into().unwrap());
                decode_name(&mut entry.name, name_seed.wrapping_add(i as u32));
            }
            entries.remove(0);
            Some(cipher)
        } else {
            None
        };
        directory::validate_ranges(&entries, bytes.len())?;
        Ok(Self {
            bytes,
            directory: ArchiveDirectory::new(entries)?,
            cipher,
        })
    }

    pub fn directory(&self) -> &ArchiveDirectory {
        &self.directory
    }

    pub fn is_encrypted(&self) -> bool {
        self.cipher.is_some()
    }

    pub fn read(&self, name: &[u8]) -> Result<Option<Vec<u8>>> {
        self.directory
            .find(name)?
            .map(|e| self.read_entry(e))
            .transpose()
    }

    /// 从所给范围读取，完整八字节块解密，尾部不足八字节时保留原值。
    pub fn read_entry(&self, entry: &DirectoryEntry) -> Result<Vec<u8>> {
        let length = read::sized(entry.length, entry.offset as usize)?;
        let mut data = read::range(self.bytes, entry.offset as usize, length)?.to_vec();
        if let Some(cipher) = &self.cipher {
            for block in data.chunks_exact_mut(8) {
                cipher.decrypt_block(block.into());
            }
        }
        Ok(data)
    }
}

fn decode_name(name: &mut [u8], seed: u32) {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let shift = ScriptRandom::seeded(seed)
        .next_integer()
        .to_le_bytes()
        .into_iter()
        .map(u32::from)
        .sum::<u32>() as u8 as usize;
    for (i, byte) in name.iter_mut().enumerate() {
        if let Some(index) = ALPHABET.iter().position(|c| c == byte) {
            let index = (index + shift + i) % 52;
            *byte = ALPHABET[51 - index];
        }
    }
}
