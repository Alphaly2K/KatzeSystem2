use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    language::LanguageState,
};

#[test]
fn ordinary_language_writes_preserve_raw_selections_and_only_mark_configuration_changes() {
    use katze_system2::language::{LanguageChangeRequest, LanguageTarget::*};
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    host.set_task_id(42);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xaa; 64], 0).unwrap(),
        vec![0xbb; 32],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, mode: u32, value: u32| {
        host.call(
            1053,
            &[mode.to_le_bytes(), value.to_le_bytes()].concat(),
            vm,
        )
    };
    assert_eq!(
        call(&mut host, &mut vm, 3, 99).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(
        call(&mut host, &mut vm, 0, 0).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(host.languages.primary_index, 0);
    assert!(host.language_change_requests.is_empty());
    assert_eq!(
        call(&mut host, &mut vm, 0, 1).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(host.languages.primary_index, 1);
    assert_eq!(host.system_settings_dirty, 1);
    assert_eq!(host.language_change_requests.len(), 1);
    host.languages.primary_index = 0;
    host.language_change_requests.clear();
    assert!(host.call(1053, &[0; 4], &mut vm).is_err());
    let mut settings = katze_system2::settings::SystemSettings::default();
    settings.sound[0x15d4..0x15d8].copy_from_slice(&50u32.to_le_bytes());
    settings.sound[0x15d8..0x15dc].copy_from_slice(&30u32.to_le_bytes());
    host.system_settings = Some(settings);
    host.system_settings_dirty = 37;
    host.languages.prefixes = vec![b"default".to_vec()];
    for (mode, value, changed) in [
        (0, u32::MAX, true),
        (0, u32::MAX, false),
        (1, 0x8000_0000, true),
        (1, 0x8000_0000, false),
        (2, 0x8000_0001, true),
        (2, 0x8000_0001, false),
        (99, 7, false),
    ] {
        host.system_settings.as_mut().unwrap().sound[..16].fill(0x55);
        host.system_settings_dirty = 37;
        let before = host.system_settings.clone();
        assert_eq!(
            call(&mut host, &mut vm, mode, value).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(host.system_settings, before);
        assert_eq!(host.system_settings_dirty, if changed { 1 } else { 37 });
    }
    assert_eq!(
        [
            host.languages.primary_index,
            host.languages.message_index,
            host.languages.subtitle_index
        ],
        [u32::MAX, 0x8000_0000, 0x8000_0001]
    );
    assert_eq!(host.languages.prefix(0), Some(b"default".as_slice()));
    assert_eq!(host.languages.prefix(1), Some(b"default".as_slice()));
    assert_eq!(
        host.language_change_requests,
        [Interface, MainText, Subtitle].map(|target| LanguageChangeRequest {
            task_id: 42,
            target
        })
    );
    assert_eq!(vm.read(0, 64).unwrap(), [0xaa; 64]);
    assert_eq!(vm.read(0x8000_0000, 32).unwrap(), [0xbb; 32]);
}

#[test]
fn language_native_preserves_mode_fallback_cascade_and_atomic_vm_writes() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    host.languages = LanguageState {
        prefixes: vec![b"aa".to_vec(), b"%LANG_M%".to_vec(), b"zz".to_vec()],
        default_index: 2,
        primary_index: 1,
        message_index: u32::MAX,
        ..Default::default()
    };
    assert_eq!(
        host.languages
            .expand_image(b"%LANG%/%LANG_M%/%lang%\0ignored")
            .unwrap(),
        b"zz/zz/%lang%"
    );
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xcc; 128], 0).unwrap(),
        vec![0xdd; 64],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, mode: u32, address: u32| {
        host.call(
            1062,
            &[mode.to_le_bytes(), address.to_le_bytes()].concat(),
            vm,
        )
    };
    for address in [0, 0x80000000] {
        vm.write(address, b"%LANG%:%LANG%:%LANG_M%\0tail").unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 0, address).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(
            vm.read_string(address).unwrap(),
            b"%LANG_M%:%LANG_M%:%LANG_M%"
        );
        call(&mut host, &mut vm, 1, address).unwrap();
        assert_eq!(vm.read_string(address).unwrap(), b"zz:zz:zz");
    }
    for mode in [2, u32::MAX] {
        assert_eq!(
            call(&mut host, &mut vm, mode, u32::MAX).unwrap(),
            NativeReturn::Continue
        );
    }
    host.languages.default_index = u32::MAX;
    assert_eq!(host.languages.expand(b"%LANG_M%", 1).unwrap(), b"%LANG_M%");
    host.languages.primary_index = 0;
    host.languages.prefixes[0] = vec![b'x'; 32];
    vm.write(112, b"%LANG%\0").unwrap();
    let before = vm.read(112, 16).unwrap().to_vec();
    assert!(call(&mut host, &mut vm, 0, 112).is_err());
    assert_eq!(vm.read(112, 16).unwrap(), before);
    host.languages.prefixes[0] = vec![b'x'; 255];
    assert!(host.languages.expand(&b"%LANG%".repeat(9), 0).is_err());
    assert!(host.call(1062, &[0; 4], &mut vm).is_err());
}

#[test]
fn prefix_query_keeps_cp932_fallback_empty_values_and_ordered_overlapping_outputs() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    host.languages = LanguageState {
        prefixes: vec![
            b"abc\0tail".to_vec(),
            b"XY".to_vec(),
            vec![0x93, 0xfa, 0x96, 0x7b],
        ],
        default_index: 2,
        primary_index: 0,
        message_index: 1,
        ..Default::default()
    };
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xcc; 64], 0).unwrap(),
        vec![0xdd; 64],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, first: u32, second: u32| {
        host.call(
            1036,
            &[first.to_le_bytes(), second.to_le_bytes()].concat(),
            vm,
        )
    };
    for (first, second, expected) in [
        (0, 0, b"XY\0\0".as_slice()),
        (0, 1, b"aXY\0"),
        (1, 0, b"XY\0c\0"),
    ] {
        vm.write(0, &[0xcc; 64]).unwrap();
        assert_eq!(
            call(&mut host, &mut vm, first, second).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(vm.read(0, expected.len()).unwrap(), expected);
    }
    host.languages.primary_index = u32::MAX;
    for unicode in [0, 1, u32::MAX] {
        host.unicode_text = unicode;
        call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
        assert_eq!(vm.read_string(0).unwrap(), [0x93, 0xfa, 0x96, 0x7b]);
        assert_eq!(vm.read_string(0x8000_0000).unwrap(), b"XY");
    }
    host.languages.prefixes[1].clear();
    call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
    assert!(vm.read_string(0x8000_0000).unwrap().is_empty());
    host.languages.default_index = u32::MAX;
    call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
    assert!(vm.read_string(0).unwrap().is_empty());
    vm.write(0, b"keep\0").unwrap();
    assert!(call(&mut host, &mut vm, 0, u32::MAX).is_err());
    assert_eq!(vm.read_string(0).unwrap(), b"keep");
    assert!(host.call(1036, &[0; 4], &mut vm).is_err());
}

#[test]
fn startup_language_block_restores_ordered_prefixes_and_default_selection() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<document>
  <language>
    <default>1</default>
    <L00><code>1028</code><prefix>cn</prefix><lang>甲</lang><accel>C</accel></L00>
    <L01><code>1033</code><prefix>en</prefix></L01>
    <L02><prefix></prefix></L02>
    <L04><prefix>late</prefix></L04>
  </language>
</document>"#;
    let state = LanguageState::from_xml(xml.as_bytes()).unwrap();
    assert_eq!(state.prefixes, [b"cn".as_slice(), b"en", b""]);
    assert_eq!(state.default_index, 1);
    assert_eq!(state.primary_index, 1);
    assert_eq!(state.message_index, 1);
    assert_eq!(state.expand(b"a/%LANG%/b\0tail", 0).unwrap(), b"a/en/b");

    // 无 language 段保留空表与零编号。
    let state = LanguageState::from_xml(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><other></other></document>",
    )
    .unwrap();
    assert!(state.prefixes.is_empty());
    assert_eq!(state.default_index, 0);
    assert_eq!(state.expand(b"%LANG%", 0).unwrap(), b"%LANG%");

    assert!(LanguageState::from_xml(b"not xml").is_err());
    assert!(LanguageState::from_xml(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><language><L00></L00></language></document>"
    )
    .is_ok());
}

#[test]
fn user_language_settings_normalize_on_load_and_export_raw_selections() {
    use katze_system2::resource::{XmlDocument, XmlWriter};
    let mut state = LanguageState {
        prefixes: vec![b"first".to_vec(), b"second".to_vec(), vec![]],
        default_index: 1,
        primary_index: 0,
        message_index: 1,
        subtitle_index: u32::MAX,
    };
    let prefixes = state.prefixes.clone();
    state
        .apply_user_xml(
            br#"<?xml version="1.0" encoding="UTF-8"?><document>
      <language><id><UI>-2147483648</UI><main>2</main><sub>7</sub></id></language>
    </document>"#,
        )
        .unwrap();
    assert_eq!(state.selection(), [1, 2, 1]);
    state.primary_index = 0x8000_0000;
    state.message_index = u32::MAX;
    state.subtitle_index = u32::MAX;
    let mut writer = XmlWriter::new();
    state.write_xml(&mut writer).unwrap();
    let bytes = writer.to_bytes().unwrap();
    let xml = XmlDocument::parse(&bytes).unwrap();
    assert_eq!(
        xml.text(b"language/id/UI").unwrap(),
        Some(&b"-2147483648"[..])
    );
    assert_eq!(state.selection(), [0x8000_0000, u32::MAX, u32::MAX]);
    state.apply_user_xml(&bytes).unwrap();
    assert_eq!(state.selection(), [1, 1, u32::MAX]);
    state.default_index = 0x8000_0001;
    state.primary_index = 99;
    state.message_index = 99;
    state.subtitle_index = 99;
    state
        .apply_user_xml(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document></document>")
        .unwrap();
    assert_eq!(state.selection(), [0x8000_0001; 3]);
    assert_eq!(state.prefixes, prefixes);
    assert_eq!(state.default_index, 0x8000_0001);
}
