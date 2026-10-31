use katze_system2::resource::{KifArchive, verification_seed};

#[test]
fn encrypted_archive_decrypts_names_ranges_and_blocks_and_preserves_clear_tails() {
    // 固定输入由独立 Blowfish 实现生成，只包含合成名称和正文。
    let bytes = include_bytes!("fixtures/encrypted.kif");
    let seed = verification_seed(b"SYNTHETIC\0ignored");
    let archive = KifArchive::parse(bytes, seed).unwrap();
    assert!(archive.is_encrypted());
    let entries = archive.directory().entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, b"sample.kcs");
    assert_eq!((entries[0].offset, entries[0].length), (296, 9));
    assert_eq!(archive.read(b"SAMPLE.KCS").unwrap().unwrap(), b"012345678");
    assert_eq!(archive.read(b"empty.kcs").unwrap().unwrap(), b"");
    assert_eq!(
        archive.read(b"tail.kcs").unwrap().unwrap(),
        b"ABCDEFGHIJKLMNOPQ"
    );
    assert!(archive.read(b"missing.kcs").unwrap().is_none());
    assert!(archive.read(b"__key__.dat").unwrap().is_none());
    assert!(KifArchive::parse(&bytes[..bytes.len() - 1], seed).is_err());
}

#[test]
fn plain_archives_return_unmodified_entry_bytes() {
    let mut bytes = b"KIF\0".to_vec();
    bytes.extend(1u32.to_le_bytes());
    let mut name = [0; 64];
    name[..6].copy_from_slice(b"sample");
    bytes.extend(name);
    bytes.extend(80u32.to_le_bytes());
    bytes.extend(4u32.to_le_bytes());
    bytes.extend(b"data");
    let archive = KifArchive::parse(&bytes, 0).unwrap();
    assert!(!archive.is_encrypted());
    assert_eq!(archive.read(b"SAMPLE").unwrap().unwrap(), b"data");
    assert_eq!(verification_seed(b""), u32::MAX);
}
