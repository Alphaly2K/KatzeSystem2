use blowfish::{
    BlowfishLE,
    cipher::{BlockEncrypt, KeyInit},
};
use katze_system2::{
    Result,
    io::{ReadSource, SharedSource},
    resource::KifEntrySource,
};
use std::sync::Arc;

struct Short(Vec<u8>);
impl ReadSource for Short {
    fn len(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
        let bytes = self.0.get(offset as usize..).unwrap_or_default();
        let count = bytes.len().min(output.len()).min(3);
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

#[test]
fn random_entry_reads_decrypt_relative_blocks_preserve_short_tails_and_bound_buffers() {
    let plain: Vec<_> = (0..40005).map(|index| (index * 37 + 13) as u8).collect();
    for key in [None, Some(0x12345678u32)] {
        let mut bytes = plain.clone();
        if let Some(key) = key {
            let cipher = BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap();
            for block in bytes.chunks_exact_mut(8) {
                cipher.encrypt_block(block.into());
            }
        }
        let bytes = [vec![0xcc; 7], bytes, vec![0xee; 13]].concat();
        let length = bytes.len() as u64;
        let parent = Arc::new(Short(bytes)) as SharedSource;
        assert!(KifEntrySource::new(parent.clone(), 8, length as u32, key).is_err());
        assert!(KifEntrySource::new(parent.clone(), u64::MAX, 8, key).is_err());
        let entry = KifEntrySource::new(parent, 7, plain.len() as u32, key).unwrap();
        assert_eq!(entry.len().unwrap(), plain.len() as u64);
        for offset in [0, 1, 7, 8, 13, 32760, 39999, 40004, 40005, 40012] {
            for size in [0, 1, 7, 8, 9, 32000, 40016] {
                let mut output = vec![0xa5; size];
                let count = entry.read_at(offset, &mut output).unwrap();
                let expected = plain.get(offset as usize..).unwrap_or_default();
                assert!(count <= expected.len().min(size));
                assert_eq!(&output[..count], &expected[..count]);
                assert!(output[count..].iter().all(|&byte| byte == 0xa5));
            }
        }
        let mut full = vec![0; plain.len()];
        entry.read_exact_at(0, &mut full).unwrap();
        assert_eq!(full, plain);
    }
}

#[test]
fn bulk_encrypted_reads_keep_block_boundaries_and_commit_the_prefix_on_failure() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Limited {
        bytes: Vec<u8>,
        limit: AtomicUsize,
    }
    impl ReadSource for Limited {
        fn len(&self) -> Result<u64> {
            Ok(self.bytes.len() as u64)
        }
        fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
            self.bytes[..self.limit.load(Ordering::Relaxed)].read_at(offset, output)
        }
    }

    let length = (2 << 20) + 37;
    let plain: Vec<_> = (0..length).map(|i| (i * 37 + i / 113) as u8).collect();
    let key = 0x12345678u32;
    let cipher = BlowfishLE::new_from_slice(&key.to_le_bytes()).unwrap();
    let mut bytes = [vec![0; 11], plain.clone()].concat();
    for block in bytes[11..].chunks_exact_mut(8) {
        cipher.encrypt_block(block.into());
    }
    let parent = Arc::new(Limited {
        limit: AtomicUsize::new(bytes.len()),
        bytes,
    });
    let entry = KifEntrySource::new(parent.clone(), 11, length as u32, Some(key)).unwrap();
    let mut output = vec![0xa5; length - 3];
    assert_eq!(entry.read_at(3, &mut output).unwrap(), 32 * 1024);
    entry.read_exact_at(3, &mut output).unwrap();
    assert_eq!(output, &plain[3..]);

    let prefix = 3 * 32 * 1024;
    let limit = 11 + prefix + 123;
    parent.limit.store(limit, Ordering::Relaxed);
    output.fill(0xa5);
    let error = entry.read_exact_at(3, &mut output).unwrap_err();
    assert_eq!(error.offset, limit);
    assert_eq!(&output[..prefix], &plain[3..3 + prefix]);
    assert!(output[prefix..].iter().all(|&byte| byte == 0xa5));
}
