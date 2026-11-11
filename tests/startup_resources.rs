#[allow(dead_code)]
#[path = "../examples/support/mod.rs"]
mod support;

use katze_system2::{kcs::ResourceServices, resource::ResourceMode};
use std::path::PathBuf;

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
