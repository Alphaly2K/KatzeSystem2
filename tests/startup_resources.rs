#[allow(dead_code)]
#[path = "../examples/support/mod.rs"]
mod support;

use katze_system2::{kcs::ResourceServices, resource::ResourceMode};
use std::path::PathBuf;

use blowfish::{
    BlowfishLE,
    cipher::{BlockEncrypt, KeyInit},
};

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut output = b"KIF\0".to_vec();
    output.extend((entries.len() as u32).to_le_bytes());
    let mut offset = 8 + entries.len() as u32 * 72;
    for (name, bytes) in entries {
        let mut record = [0; 72];
        record[..name.len()].copy_from_slice(name.as_bytes());
        record[64..68].copy_from_slice(&offset.to_le_bytes());
        record[68..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        output.extend(record);
        offset += bytes.len() as u32;
    }
    for (_, bytes) in entries {
        output.extend(bytes);
    }
    output
}

fn program(control: u32) -> Vec<u8> {
    let mut output = b"KCS\0".to_vec();
    for word in [2, control, 52, 0, 52, 0, 0, 0, 64, 52, 0, 0] {
        output.extend(word.to_le_bytes());
    }
    let encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    output.extend(encoder.finish().unwrap());
    output
}

#[test]
fn startup_discovers_patch_priority_and_preloads_overridden_and_added_programs() {
    let root = Directory(
        std::env::temp_dir().join(format!("katze-startup-patches-{}", std::process::id())),
    );
    std::fs::create_dir(&root.0).unwrap();
    std::fs::create_dir(root.0.join("config")).unwrap();
    std::fs::create_dir(root.0.join("loose")).unwrap();
    std::fs::write(root.0.join("config/startup.xml"), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><SCRIPT><start>kcs.int/main.kcs</start></SCRIPT><INT><int00><name>kcs</name><int>kcs.int</int><ext>kcs</ext><dir>loose</dir></int00></INT></document>").unwrap();
    std::fs::write(root.0.join("loose/shared.dat"), b"loose").unwrap();
    for (name, entries) in [
        (
            "kcs.int",
            vec![("main.kcs", program(0)), ("base.dat", b"base".to_vec())],
        ),
        (
            "update01.int",
            vec![("shared.dat", b"lower".to_vec()), ("lower.dat", Vec::new())],
        ),
        (
            "UPDATE12.INT",
            vec![
                ("shared.dat", b"higher".to_vec()),
                ("main.kcs", program(12)),
                ("extra.kcs", program(13)),
            ],
        ),
    ] {
        std::fs::write(root.0.join(name), archive(&entries)).unwrap();
    }
    std::fs::write(root.0.join("updatex1.int"), b"ignored").unwrap();
    std::fs::write(root.0.join("update99.int"), archive(&[])).unwrap();
    let mut game = support::load_game(root.0.to_str().unwrap()).unwrap();
    assert_eq!(game.router.patches.len(), 3);
    assert_eq!(game.programs["main"].control, 12);
    assert_eq!(game.programs["extra.kcs"].control, 13);
    for mode in [
        ResourceMode::File,
        ResourceMode::LooseFirst,
        ResourceMode::ArchiveFirst,
    ] {
        for name in ["shared.dat", "kcs.int/shared.dat", "other/shared.dat"] {
            assert_eq!(
                game.router.read(name.as_bytes(), mode).unwrap().unwrap(),
                b"higher"
            );
            assert_eq!(
                game.router
                    .read_utf16(&name.encode_utf16().collect::<Vec<_>>(), mode)
                    .unwrap()
                    .unwrap(),
                b"higher"
            );
        }
    }
    assert_eq!(
        game.router
            .read(b"kcs.int/base.dat", ResourceMode::ArchiveFirst)
            .unwrap()
            .unwrap(),
        b"base"
    );
    assert_eq!(
        game.router
            .read(b"kcs.int/lower.dat", ResourceMode::LooseFirst)
            .unwrap(),
        Some(Vec::new())
    );
}

#[test]
fn encrypted_archive_lookup_preserves_all_name_shifts_and_lossy_unicode_matching() {
    let root = Directory(
        std::env::temp_dir().join(format!("katze-startup-encrypted-{}", std::process::id())),
    );
    std::fs::create_dir(&root.0).unwrap();
    let key = katze_system2::kcs::ScriptRandom::seeded(23)
        .next_integer()
        .to_le_bytes();
    let cipher = BlowfishLE::new_from_slice(&key).unwrap();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    for (case, raw) in [
        b"Voice_0123.ogg".as_slice(),
        b"123_45.678",
        b".a_B-9",
        "a音b".as_bytes(),
        b"a\xffb",
        b"",
    ]
    .into_iter()
    .enumerate()
    {
        let mut directory = [0u8; 144];
        directory[..11].copy_from_slice(b"__key__.dat");
        directory[68..72].copy_from_slice(&23u32.to_le_bytes());
        directory[72..72 + raw.len()].copy_from_slice(raw);
        let mut block = [0u8; 8];
        block[..4].copy_from_slice(&152u32.to_le_bytes());
        block[4..].copy_from_slice(&8u32.to_le_bytes());
        cipher.encrypt_block((&mut block).into());
        let offset = u32::from_le_bytes(block[..4].try_into().unwrap()).wrapping_sub(1);
        directory[136..140].copy_from_slice(&offset.to_le_bytes());
        directory[140..].copy_from_slice(&block[4..]);
        let payload = *b"contents";
        let mut data = payload;
        cipher.encrypt_block((&mut data).into());
        let bytes = [b"KIF\0".as_slice(), &2u32.to_le_bytes(), &directory, &data].concat();
        let path = root.0.join(format!("{case}.int"));
        std::fs::write(&path, bytes).unwrap();
        let archive = support::Archive::open(path.to_str().unwrap()).unwrap();
        for shift in 0..52 {
            let decoded: Vec<_> = raw
                .iter()
                .enumerate()
                .map(|(i, &byte)| {
                    alphabet
                        .iter()
                        .position(|&letter| letter == byte)
                        .map_or(byte, |index| alphabet[51 - (index + shift + i) % 52])
                })
                .collect();
            let name = String::from_utf8_lossy(&decoded);
            for request in [
                name.to_string(),
                name.to_ascii_uppercase(),
                name.to_ascii_lowercase(),
            ] {
                assert_eq!(
                    archive
                        .open_source(&request)
                        .unwrap()
                        .map(support::read_source)
                        .transpose()
                        .unwrap()
                        .as_deref(),
                    Some(payload.as_slice())
                );
                assert!(
                    archive
                        .open_source(&format!("{request}!"))
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}
