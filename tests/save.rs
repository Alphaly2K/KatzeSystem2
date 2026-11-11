use katze_system2::save::DataContainer;
use std::io::Write;

fn css(records: &[u8], used: u32, tag: u32) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(records).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut file = b"CSS\0".to_vec();
    for n in [compressed.len() as u32, used, tag] {
        file.extend(n.to_le_bytes());
    }
    file.extend(compressed);
    file
}

#[test]
fn records_keep_duplicates_and_delete_only_the_first_without_resetting_the_tag() {
    let mut data = DataContainer::new(0xfedc_ba98);
    data.append(7, b"abc").unwrap();
    data.append(7, b"xyz123").unwrap();
    data.append(8, b"").unwrap();
    assert_eq!(data.used_len(), 33);
    assert_eq!(data.record(7).unwrap(), Some(b"abc".as_slice()));
    assert_eq!(data.record(8).unwrap(), Some(b"".as_slice()));
    assert!(data.remove(7).unwrap());
    assert_eq!((data.capacity(), data.used_len()), (0x80000 - 11, 22));
    assert_eq!(data.record(7).unwrap(), Some(b"xyz123".as_slice()));
    assert!(!data.remove(999).unwrap());
    data.append(9, &vec![0xa5; 0x80000]).unwrap();
    assert_eq!(data.capacity(), 0x100000 - 11);
    assert_eq!(data.record(9).unwrap().unwrap(), vec![0xa5; 0x80000]);
    data.clear();
    assert_eq!(data.tag(), 0xfedc_ba98);
    assert_eq!((data.capacity(), data.used_len()), (0x80000, 0));
    assert!(data.used_bytes().unwrap().is_empty());
    assert_eq!(data.record(7).unwrap(), None);
}

#[test]
fn css_retains_decompressed_records_outside_declared_usage_and_ignores_zlib_errors() {
    let records = [7u32.to_le_bytes().as_slice(), &3u32.to_le_bytes(), b"abc"].concat();
    for used in [0, 10, 11, 27, 0x80000] {
        let file = css(&records, used, 0x8000_0001);
        let loaded = DataContainer::from_css(&file).unwrap().unwrap();
        assert_eq!(loaded.used_len(), used);
        assert_eq!(loaded.tag(), 0x8000_0001);
        assert_eq!(loaded.record(7).unwrap(), Some(b"abc".as_slice()));
        assert_eq!(
            loaded.capacity(),
            if used == 0x80000 { 0x100000 } else { 0x80000 }
        );
    }
    let mut broken_checksum = css(&records, 11, 9);
    *broken_checksum.last_mut().unwrap() ^= 1;
    assert_eq!(
        DataContainer::from_css(&broken_checksum)
            .unwrap()
            .unwrap()
            .record(7)
            .unwrap(),
        Some(b"abc".as_slice())
    );
    let mut trailing = css(&records, 11, 9);
    let compressed = (trailing.len() - 16 + 4) as u32;
    trailing[4..8].copy_from_slice(&compressed.to_le_bytes());
    trailing.extend(b"junk");
    assert_eq!(
        DataContainer::from_css(&trailing)
            .unwrap()
            .unwrap()
            .record(7)
            .unwrap(),
        Some(b"abc".as_slice())
    );
    assert!(DataContainer::from_css(b"BAD!").unwrap().is_none());
    assert!(DataContainer::from_css(b"CSS\0").is_err());
    assert!(DataContainer::from_css(&trailing[..trailing.len() - 1]).is_err());
    trailing[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(DataContainer::from_css(&trailing).is_err());
}

#[test]
fn serialization_writes_only_declared_usage_and_restores_the_tag() {
    let mut data = DataContainer::new(0x1234_5678);
    data.append(1, b"alpha").unwrap();
    data.append(2, b"").unwrap();
    data.append(1, b"beta").unwrap();
    assert!(data.remove(1).unwrap());
    let file = data.to_css().unwrap();
    assert_eq!(&file[..4], b"CSS\0");
    let compressed = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    assert_eq!(compressed, file.len() - 16);
    assert_eq!(
        u32::from_le_bytes(file[8..12].try_into().unwrap()),
        data.used_len()
    );
    assert_eq!(
        u32::from_le_bytes(file[12..16].try_into().unwrap()),
        0x1234_5678
    );
    let loaded = DataContainer::from_css(&file).unwrap().unwrap();
    assert_eq!(loaded.tag(), 0x1234_5678);
    assert_eq!(loaded.used_len(), data.used_len());
    assert_eq!(loaded.used_bytes().unwrap(), data.used_bytes().unwrap());
    assert_eq!(loaded.record(1).unwrap(), Some(b"beta".as_slice()));
    assert_eq!(loaded.record(2).unwrap(), Some(b"".as_slice()));
    // 删除缩短的逻辑容量不进入文件头，重新加载按声明用量取整。
    assert_eq!(loaded.capacity(), 0x80000);

    let records = [7u32.to_le_bytes().as_slice(), &3u32.to_le_bytes(), b"abc"].concat();
    let beyond = DataContainer::from_css(&css(&records, 0, 9))
        .unwrap()
        .unwrap();
    let reloaded = DataContainer::from_css(&beyond.to_css().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.used_len(), 0);
    assert_eq!(beyond.record(7).unwrap(), Some(b"abc".as_slice()));
    assert_eq!(reloaded.record(7).unwrap(), None);

    let empty = DataContainer::new(0);
    let reloaded = DataContainer::from_css(&empty.to_css().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!((reloaded.capacity(), reloaded.used_len()), (0x80000, 0));
    assert_eq!(reloaded.record(0).unwrap(), empty.record(0).unwrap());
}

#[test]
fn deletion_keeps_an_exact_capacity_record_and_malformed_payloads_are_bounded() {
    let mut records = [7u32.to_le_bytes(), (0x80000u32 - 8).to_le_bytes()].concat();
    records.resize(0x80000, 0xa5);
    let mut data = DataContainer::from_css(&css(&records, 0, 1))
        .unwrap()
        .unwrap();
    assert_eq!(data.record(7).unwrap().unwrap().len(), 0x80000 - 8);
    assert!(!data.remove(7).unwrap());
    assert_eq!((data.capacity(), data.used_len()), (0x80000, 0));
    records[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let data = DataContainer::from_css(&css(&records[..8], 8, 1))
        .unwrap()
        .unwrap();
    assert!(data.record(7).is_err());
    assert!(data.record(8).unwrap().is_none());
}
