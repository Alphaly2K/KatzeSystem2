#[allow(dead_code)]
#[path = "../examples/support/mod.rs"]
mod support;

use katze_system2::{
    host::resources::{ResourceMode, ResourceServices},
    runtime::TaskRuntime,
    task::{Command, CommandBuffers},
};
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
fn startup_and_dynamic_script_loads_use_the_higher_priority_patch() {
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
    let mut runtime = TaskRuntime::prepare(0, 0).unwrap();
    runtime.set_resources(game.router);
    let (mut runtime, tasks) = runtime
        .start_game_with_tasks(game.start.as_bytes())
        .unwrap();
    assert_eq!(
        runtime
            .object(tasks.entry)
            .unwrap()
            .interpreter()
            .unwrap()
            .program()
            .control,
        12
    );
    let target = runtime.create_unloaded_ordinary(0, b"").unwrap();
    let mut name = *b"kcs.int/extra.kcs\0";
    runtime
        .dispatch_command_with_buffers(
            tasks.entry,
            Command {
                target,
                sender: tasks.entry,
                code: 161,
                data: [0; 3],
            },
            &mut CommandBuffers {
                data1: Some(&mut name),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .object(target)
            .unwrap()
            .interpreter()
            .unwrap()
            .program()
            .control,
        13
    );
}

#[test]
fn archive_list_mounts_each_member_in_order_with_its_own_loose_directory() {
    use ResourceMode::{Archive, ArchiveFirst, File, LooseFirst};

    let root =
        Directory(std::env::temp_dir().join(format!("katze-startup-lists-{}", std::process::id())));
    std::fs::create_dir(&root.0).unwrap();
    std::fs::create_dir(root.0.join("config")).unwrap();
    std::fs::create_dir_all(root.0.join("loose/first")).unwrap();
    std::fs::create_dir(root.0.join("loose/second")).unwrap();
    std::fs::write(
        root.0.join("config/startup.xml"),
        br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <SCRIPT><start>kcs.int/main.kcs</start></SCRIPT>
        <INT><int00><name>image</name><int>pack.lst</int><ext>dat</ext><dir>unrelated</dir></int00></INT>
        </document>"#,
    ).unwrap();
    std::fs::write(
        root.0.join("pack.lst"),
        b"loose\\first/first.int\r\nloose\\second/missing.int\n/last.int\0ignore",
    )
    .unwrap();
    for (path, bytes) in [
        ("loose/first/common.dat", b"loose".as_slice()),
        ("loose/first/interleaved.dat", b"first loose"),
        ("loose/second/loose-only.dat", b"second loose"),
        ("loose/first/patched.dat", b"old"),
        ("direct.dat", b"direct"),
    ] {
        std::fs::write(root.0.join(path), bytes).unwrap();
    }
    for (path, entries) in [
        ("first.int", vec![("common.dat", b"first".to_vec())]),
        (
            "last.int",
            vec![
                ("common.dat", b"last".to_vec()),
                ("interleaved.dat", b"last".to_vec()),
                ("suffix.dat", b"last".to_vec()),
            ],
        ),
        ("update01.int", vec![("patched.dat", b"patch".to_vec())]),
    ] {
        std::fs::write(root.0.join(path), archive(&entries)).unwrap();
    }
    let mut router = support::load_game(root.0.to_str().unwrap()).unwrap().router;
    assert_eq!(
        router.resolve(b"image", b"common").unwrap(),
        b"pack.lst/common.dat"
    );
    for (mode, name, expected) in [
        (LooseFirst, "PACK.LST/common.dat", Some(b"loose".as_slice())),
        (ArchiveFirst, "pack.lst/common.dat", Some(b"first")),
        (Archive, "pack.lst/common.dat", Some(b"first")),
        (
            ArchiveFirst,
            "pack.lst/interleaved.dat",
            Some(b"first loose"),
        ),
        (Archive, "pack.lst/interleaved.dat", Some(b"last")),
        (LooseFirst, "pack.lst/loose-only.dat", Some(b"second loose")),
        (Archive, "pack.lst/loose-only.dat", None),
        (Archive, "pack.lst/suffix.dat", Some(b"last")),
        (File, "pack.lst/common.dat", None),
        (File, "direct.dat", Some(b"direct")),
        (File, "pack.lst/patched.dat", Some(b"patch")),
        (Archive, "pack.lst/patched.dat", Some(b"patch")),
        (LooseFirst, "pack.lst/patched.dat", Some(b"patch")),
        (ArchiveFirst, "pack.lst/patched.dat", Some(b"patch")),
    ] {
        let units: Vec<_> = name.encode_utf16().collect();
        let source = router.open_utf16(&units, mode).unwrap();
        assert_eq!(
            source
                .map(support::read_source)
                .transpose()
                .unwrap()
                .as_deref(),
            expected,
            "{mode:?} {name}"
        );
    }
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
