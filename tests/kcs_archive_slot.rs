use katze_system2::{
    Result,
    audio::{SoundConfig, SoundPart},
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
    resource::ArchiveConfig,
    settings::SystemSettings,
};

fn instruction(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

#[test]
fn archive_slots_expand_file_before_category_and_preserve_sound_lookup_modes() -> Result<()> {
    let mut data = vec![0; 256];
    data[16..20].copy_from_slice(&7u32.to_le_bytes());
    for (at, text) in [
        (64, b"voice-%u[G16]\0".as_slice()),
        (128, b"SOUND\0"),
        (160, b"IMAGE\0"),
        (224, b"missing\0"),
        (240, b"other\0"),
    ] {
        data[at..at + text.len()].copy_from_slice(text);
    }
    for external in [0, 0x8000_0000] {
        let mut code = Vec::new();
        instruction(&mut code, 0x25, &[0x1357_9bdf]);
        for (file, category, output) in [
            (64 | external, 128, 0),
            (192, 160 | external, 4),
            (64, 208 | external, 8),
            (64, 224, 12),
            (240 | external, 128, 20),
        ] {
            instruction(&mut code, 0x25, &[file]);
            instruction(&mut code, 0x25, &[category]);
            instruction(&mut code, 0x2b, &[674, 8]);
            instruction(&mut code, 0x26, &[output]);
        }
        code.push(0x3b);
        let program = Program::from_code(&code, data.clone(), 64)?;
        for (archive, expected) in [(b"voice".as_slice(), 1u32), (b"", 4)] {
            for (new_sound, budget) in [(false, 1), (false, 4096), (true, 1), (true, 4096)] {
                let mut config = ArchiveConfig::default();
                for (slot, name) in [
                    (0, b"image".as_slice()),
                    (1, b"Voice"),
                    (2, b"voice"),
                    (3, b"other"),
                ] {
                    config.entries[slot].name = name.to_vec();
                }
                let mut settings = SystemSettings::default();
                settings.sound[60..65].copy_from_slice(b"voice");
                settings.sound[104..104 + archive.len()].copy_from_slice(archive);
                let mut state = State::new(0);
                let mut host = CoreHost::new(&mut state);
                host.archive_config = Some(config);
                host.system_settings = Some(settings);
                host.sound_config_enabled = u32::from(new_sound);
                host.sound_config = Some(SoundConfig {
                    parts: vec![SoundPart {
                        file_prefix: b"voice".to_vec(),
                        archive: archive.to_vec(),
                        ..Default::default()
                    }],
                    ..Default::default()
                });
                let mut task = OrdinaryTask::new(program.clone(), data.clone())?;
                let mut done = false;
                for _ in 0..64 {
                    done = task.update(0, 1, &mut host, budget)?;
                    if done {
                        break;
                    }
                }
                assert!(done);
                let words: Vec<_> = task
                    .interpreter
                    .read(0, 24)?
                    .chunks_exact(4)
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                    .collect();
                assert_eq!(words, [expected, 0, 4, u32::MAX, 7, u32::MAX]);
                assert_eq!(task.interpreter.read(32, 224)?, data[32..]);
                assert_eq!(task.interpreter.read(0x8000_0000, 256)?, data);
                assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
            }
        }
    }
    Ok(())
}
