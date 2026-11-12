use katze_system2::backlog::{Backlog, Record, Settings};

fn record<'a>(input: u32, name: &'a [u8], text: &'a [u8]) -> Record<'a> {
    Record {
        file: b"scene.cst",
        input,
        name,
        text,
        flag: 1,
        space: 1,
        extra: u32::MAX,
    }
}

#[test]
fn retained_history_preserves_tables_and_absolute_positions_across_restore() {
    let settings = Settings {
        columns: 8,
        space: 1,
        name_prefix: b"<".to_vec(),
        name_suffix: b">".to_vec(),
        ..Default::default()
    };
    let mut log = Backlog::new(settings.clone());
    log.push(record(0, b"Alice", b"1234567890")).unwrap();
    assert!(!log.push(record(0, b"ignored", b"duplicate")).unwrap());
    assert_eq!(log.row(0), Some(b"<Alice>".as_slice()));
    assert_eq!(log.row(1), Some(b"1234567".as_slice()));
    assert_eq!(log.row(2), Some(b"890".as_slice()));
    log.push(record(1, b"Bob", b"text")).unwrap();
    assert_eq!(log.row(3), Some(b"".as_slice()));
    assert_eq!(log.name_for_row(3), None);
    log.set_limit(1);
    assert_eq!(
        (log.first_group(), log.first_row(), log.row_count()),
        (1, 4, 2)
    );
    assert_eq!(log.name_for_row(1), Some(b"<Bob>".as_slice()));
    let data = log.serialize().unwrap();
    assert_eq!(
        &data[..24],
        &[24u32, 38, 70, 93, 1, 4]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    let mut restored = Backlog::new(settings);
    restored.restore(&data).unwrap();
    assert_eq!(restored.serialize().unwrap(), data);
    restored.set_limit(0);
    assert_eq!((restored.first_group(), restored.first_row()), (2, 6));
    restored.clear();
    assert_eq!(
        (
            restored.first_group(),
            restored.first_row(),
            restored.limit()
        ),
        (2, 6, 0)
    );
    restored.set_limit(1);
    restored.push(record(9, b"Carol", b"new")).unwrap();
    assert_eq!(restored.name_for_row(0), Some(b"<Carol>".as_slice()));
    let before = restored.clone();
    assert!(restored.restore(&data[..20]).is_err());
    assert_eq!(restored, before);
}

#[test]
fn log_line_rules_keep_literal_controls_and_use_byte_widths_for_brackets() {
    let mut log = Backlog::new(Settings {
        columns: 8,
        ..Default::default()
    });
    log.push(record(0, b"", b"A\\r[ab/reading]BC\\nD")).unwrap();
    assert_eq!(log.row(0), Some(b"A[ab/reading]BC\\n".as_slice()));
    assert_eq!(log.row(1), Some(b"D".as_slice()));
    log.push(record(
        1,
        b"",
        &[0x81, 0x75, b'a', b'b', b'c', b'd', b'e', b'f', b'g'],
    ))
    .unwrap();
    assert_eq!(
        log.row(2),
        Some([0x81, 0x75, b'a', b'b', b'c', b'd', b'e'].as_slice())
    );
    assert_eq!(log.row(3), Some([0x81, 0x40, b'f', b'g'].as_slice()));
    let mut plain = Backlog::new(Settings {
        escape: 0,
        ..Default::default()
    });
    plain
        .push(record(0, b"", b"A\\c0xff;B\\w+12;C\\nD\\rE\\\\F"))
        .unwrap();
    assert_eq!(plain.row(0), Some(b"ABCDrE\\\\F".as_slice()));
}

#[test]
fn startup_log_settings_preserve_utf8_name_format_and_missing_values() {
    let mut system = katze_system2::settings::SystemSettings::default();
    assert_eq!(system.backlog_static, 0);
    system.apply_startup_xml(r#"<?xml version="1.0" encoding="UTF-8"?><document><GAME><log><col>12</col><prof>2</prof><nameline>0</nameline><nameformat><first>【🙂</first><last>】</last></nameformat><nameindent>2</nameindent><space>1</space><escape>0</escape><static>2147483648</static><disp><mes>0</mes><name>2</name></disp></log></GAME></document>"#.as_bytes()).unwrap();
    let s = &system.backlog;
    assert_eq!(
        (
            s.columns,
            s.profile,
            s.name_line,
            s.indent,
            s.name_indent,
            s.space,
            s.escape
        ),
        (12, 2, 0, 1, 2, 1, 0)
    );
    assert_eq!((s.display_message, s.display_name), (0, 2));
    assert_eq!(s.name_prefix, "【🙂".as_bytes());
    assert_eq!(s.name_suffix, "】".as_bytes());
    system
        .apply_startup_xml(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document/>")
        .unwrap();
    assert_eq!(system.backlog_static, 0x8000_0000);
    use katze_system2::kcs::{CoreHost, Host, Interpreter, NativeReturn, Program};
    let mut numbers = katze_system2::expression::State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![], 16).unwrap(), vec![]).unwrap();
    assert!(host.call(852, &[], &mut vm).is_err());
    host.system_settings = Some(system);
    assert_eq!(
        host.call(852, &[], &mut vm).unwrap(),
        NativeReturn::Value(0x8000_0000)
    );
}
