use katze_system2::{
    Error,
    language::{StringTable, StringTables},
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn tables_preserve_pairing_comments_spaces_control_values_and_unicode() {
    let input = "\u{feff}// header\r\n\
        key\n first \nkey\nignored\n\
        skip\n$nr$\nskip\nkept\n\
        empty\n$empty$\nempty\nignored\n\
        \x20\x20discarded\n\t\talso discarded\n\
        quoted\n\"a//tail\n\
        joined\na/*block*/b\n\
        tabs\na\tb   \n\
        \u{40000}\nあ\u{40000}\n\
        literal\n\\n#value\n\
        orphan\0hidden\nvalue\n";
    let table = StringTable::parse(input.as_bytes()).unwrap();
    let expected = [
        ("key", "first"),
        ("skip", "kept"),
        ("empty", ""),
        ("quoted", "\"a"),
        ("joined", "ab"),
        ("tabs", "a b  "),
        ("\u{40000}", "あ\u{40000}"),
        ("literal", "\\n#value"),
    ];
    assert_eq!(table.len(), expected.len());
    for (key, value) in expected {
        assert_eq!(table.get(&wide(key)), Some(wide(value).as_slice()), "{key}");
    }
    let table = StringTable::parse(b"key\na/*block\r\nmore*/b\nlast\nend\n").unwrap();
    assert_eq!(table.get(&wide("key")), Some(wide("a").as_slice()));
    assert_eq!(table.get(&wide("b")), Some(wide("last").as_slice()));
    assert_eq!(table.len(), 2);
    assert!(
        StringTable::parse(b"/*unterminated\nkey\nvalue")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn malformed_utf8_keeps_native_replacement_boundaries() {
    for (bytes, expected) in [
        (b"\x80".as_slice(), "�"),
        (b"\xc2", "�"),
        (b"\xc0\xaf", "��"),
        (b"\xed\xa0\x80", "��"),
        (b"\xf4\x90\x80\x80", "���"),
        (b"\xe3\x81", "�"),
        (b"\xe3(\x82", "�(�"),
        (b"\xf0\x90\x80", "�"),
    ] {
        let input = [b"key\nA".as_slice(), bytes, b"Z\n"].concat();
        let table = StringTable::parse(&input).unwrap();
        assert_eq!(
            table.get(&wide("key")),
            Some(wide(&format!("A{expected}Z")).as_slice())
        );
    }
}

#[test]
fn reload_removes_old_tables_and_tag_lookup_preserves_fallbacks_and_priority() {
    let mut tables = StringTables::default();
    tables
        .reload(1, || {
            Ok(Some(
                b"$same$\nGame\n$empty$\nOther\nplain\nChanged\n".to_vec(),
            ))
        })
        .unwrap();
    tables
        .reload(0, || {
            Ok(Some(
                b"$same$\nUI\n#mark#\nHash\n$empty$\n$empty$\n$$\nEmpty key\n".to_vec(),
            ))
        })
        .unwrap();
    for (input, expected) in [
        ("$same$Fallback", "UI"),
        ("#mark#Fallback", "Hash"),
        ("$empty$Fallback", ""),
        ("$missing$Fallback", "Fallback"),
        ("$$Tail", "Empty key"),
        ("$sa$me$", "me$"),
        ("plain", "plain"),
        ("prefix$same$", "prefix$same$"),
        ("$same", "$same"),
        ("#missing##mark#Tail", "#mark#Tail"),
        ("$same$\0ignored", "UI"),
    ] {
        assert_eq!(tables.resolve(&wide(input)), wide(expected), "{input}");
    }
    assert!(!tables.reload(0, || Ok(None)).unwrap());
    assert_eq!(tables.resolve(&wide("$same$Fallback")), wide("Game"));
    assert!(tables.reload(0, || Ok(Some(Vec::new()))).unwrap());
    assert!(tables.table(0).unwrap().is_empty());
    assert_eq!(tables.resolve(&wide("$same$Fallback")), wide("Game"));
    assert!(
        tables
            .reload(1, || Err(Error::new(0, "read failed")))
            .is_err()
    );
    assert!(tables.table(1).is_none());
    assert_eq!(tables.resolve(&wide("$same$Fallback")), wide("Fallback"));
}

#[test]
fn runtime_reloads_wide_configured_paths_using_current_selections_and_file_fallback() {
    use katze_system2::{
        Result,
        expression::State,
        kcs::ResourceServices,
        language::{LanguageState, LanguageTarget::*, StringTableConfig},
        resource::{ResourceMode, XmlDocument},
        runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    };
    use std::{cell::RefCell, rc::Rc};
    struct Reject;
    impl ObjectFactory for Reject {
        fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
            unreachable!()
        }
    }
    type Reads = Rc<RefCell<Vec<(Vec<u16>, ResourceMode)>>>;
    struct Files(Reads);
    impl ResourceServices for Files {
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("wide resource paths must reach the host unchanged")
        }
        fn read_utf16(&mut self, path: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            self.0.borrow_mut().push((path.to_vec(), mode));
            Ok(match (String::from_utf16(path).unwrap().as_str(), mode) {
                ("language_甲.txt", ResourceMode::File) => Some(b"$key$\nUI\n".to_vec()),
                ("資料/甲_😀.txt", ResourceMode::LooseFirst) => {
                    Some(b"$key$\nGame\n".to_vec())
                }
                _ => None,
            })
        }
    }
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><language>\
        <default>0</default><L00><prefix>甲</prefix></L00><L01><prefix>乙</prefix></L01>\
        <OPTTABLE>資料/%LANG%_😀.txt</OPTTABLE></language></document>";
    let config = StringTableConfig::from_xml(xml.as_bytes()).unwrap();
    // 宽字符 XML 按字符块保留首单元，表内容则保留完整代理对。
    assert_eq!(
        config.game_path,
        [wide("資料/%LANG%_"), vec![0xd83d], wide(".txt")].concat()
    );
    let xml = xml.replace("😀", "%LANG_M%");
    let mut config = StringTableConfig::from_xml(xml.as_bytes()).unwrap();
    config.prefixes[1] = wide("😀");
    let reads = Reads::default();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        Reject,
    );
    assert!(runtime.reload_language_tables(Interface).is_err());
    runtime.host_mut().languages = LanguageState::from_xml(xml.as_bytes()).unwrap();
    runtime.host_mut().languages.message_index = 1;
    runtime.host_mut().string_table_config = Some(config);
    runtime.set_resources(Files(reads.clone()));
    runtime.reload_language_tables(Interface).unwrap();
    assert_eq!(
        *reads.borrow(),
        [
            (wide("language_甲.txt"), ResourceMode::File),
            (wide("資料/甲_😀.txt"), ResourceMode::File),
            (wide("資料/甲_😀.txt"), ResourceMode::LooseFirst),
        ]
    );
    assert_eq!(
        runtime.host().string_tables.resolve(&wide("$key$Fallback")),
        wide("UI")
    );
    reads.borrow_mut().clear();
    runtime.reload_language_tables(MainText).unwrap();
    assert_eq!(reads.borrow().len(), 4);
    assert!(
        reads
            .borrow()
            .iter()
            .all(|(path, _)| path == &wide("資料/甲_😀.txt"))
    );
    reads.borrow_mut().clear();
    runtime.host_mut().languages.primary_index = 99;
    runtime.reload_language_tables(Interface).unwrap();
    runtime.reload_language_tables(Subtitle).unwrap();
    assert!(reads.borrow().is_empty());
    runtime.reload_language_tables(MainText).unwrap();
    assert_eq!(reads.borrow().len(), 4);
    assert_eq!(reads.borrow()[0].0, wide("資料/甲_😀.txt"));
    reads.borrow_mut().clear();
    runtime.host_mut().languages.message_index = 99;
    runtime.reload_language_tables(MainText).unwrap();
    assert_eq!(reads.borrow().len(), 2);
    runtime.host_mut().languages.primary_index = 1;
    runtime.reload_language_tables(Interface).unwrap();
    assert_eq!(
        runtime.host().string_tables.resolve(&wide("$key$Fallback")),
        wide("Fallback")
    );
    assert!(runtime.host().string_tables.table(0).is_none());
    assert!(runtime.host().string_tables.table(1).is_none());

    let xml = XmlDocument::parse(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><empty></empty></document>",
    )
    .unwrap();
    assert_eq!(xml.wide_text(b"missing").unwrap(), None);
    assert_eq!(xml.wide_text(b"empty").unwrap(), Some(Vec::new()));
}

#[test]
fn default_wide_resource_entry_transcodes_cp932_and_rejects_unrepresentable_names() {
    use katze_system2::{Result, kcs::ResourceServices, resource::ResourceMode};
    #[derive(Default)]
    struct Files(Vec<u8>);
    impl ResourceServices for Files {
        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::File);
            self.0 = name.to_vec();
            Ok(None)
        }
    }
    let mut files = Files::default();
    let mut services = &mut files;
    ResourceServices::read_utf16(&mut services, &wide("日本\0ignored"), ResourceMode::File)
        .unwrap();
    assert_eq!(files.0, [0x93, 0xfa, 0x96, 0x7b]);
    assert!(files.read_utf16(&wide("😀"), ResourceMode::File).is_err());
    assert!(files.read_utf16(&[0xd800], ResourceMode::File).is_err());
}

#[test]
fn platform_refresh_normalizes_subtitles_and_refreshes_volumes_without_saving_or_consuming_requests()
 {
    use katze_system2::{
        Result,
        expression::State,
        kcs::ResourceServices,
        language::{LanguageChangeRequest, LanguageTarget::*, StringTableConfig},
        resource::ResourceMode,
        runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
        settings::SystemSettings,
    };
    use std::{cell::Cell, rc::Rc};
    struct Reject;
    impl ObjectFactory for Reject {
        fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
            unreachable!()
        }
    }
    struct Files(Rc<Cell<usize>>);
    impl ResourceServices for Files {
        fn read(&mut self, _: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::File);
            self.0.set(self.0.get() + 1);
            Ok(Some(Vec::new()))
        }
    }
    let reads = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        Reject,
    );
    runtime.set_resources(Files(reads.clone()));
    runtime.host_mut().languages.prefixes = vec![b"a".to_vec(), b"b".to_vec()];
    runtime.host_mut().string_table_config = Some(StringTableConfig {
        prefixes: vec![wide("a"), wide("b")],
        game_path: wide("game.txt"),
    });
    let mut settings = SystemSettings::default();
    settings.sound[0x15d4..0x15d8].copy_from_slice(&50u32.to_le_bytes());
    settings.sound[0x15d8..0x15dc].copy_from_slice(&30u32.to_le_bytes());
    runtime.host_mut().system_settings = Some(settings);
    runtime.host_mut().system_settings_dirty = 37;
    for (target, value, expected_reads, refresh) in [
        (Interface, 0, 2, true),
        (Interface, 2, 0, false),
        (MainText, 0, 2, true),
        (MainText, 2, 1, false),
        (Subtitle, 1, 0, true),
        (Subtitle, 2, 0, true),
        (Subtitle, u32::MAX, 0, true),
    ] {
        let host = runtime.host_mut();
        host.languages.primary_index = 9;
        host.languages.message_index = 10;
        host.languages.subtitle_index = 11;
        match target {
            Interface => host.languages.primary_index = value,
            MainText => host.languages.message_index = value,
            Subtitle => host.languages.subtitle_index = value,
        }
        let request = LanguageChangeRequest {
            task_id: 99,
            target,
        };
        host.language_change_requests = vec![request];
        for _ in 0..2 {
            reads.set(0);
            runtime.host_mut().system_settings.as_mut().unwrap().sound[..16].fill(0x55);
            runtime.refresh_language(target).unwrap();
            let host = runtime.host();
            assert_eq!(reads.get(), expected_reads);
            let expected = if refresh {
                [15u32, 50, 50, 50]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                vec![0x55; 16]
            };
            assert_eq!(
                &host.system_settings.as_ref().unwrap().sound[..16],
                expected
            );
            assert_eq!(host.system_settings_dirty, 37);
            assert_eq!(host.language_change_requests, [request]);
            assert_eq!(
                host.languages.primary_index,
                if target == Interface { value } else { 9 }
            );
            assert_eq!(
                host.languages.message_index,
                if target == MainText { value } else { 10 }
            );
            assert_eq!(
                host.languages.subtitle_index,
                if target == Subtitle {
                    if value < 2 { value } else { u32::MAX }
                } else {
                    11
                }
            );
        }
    }
    runtime.host_mut().system_settings = None;
    runtime.host_mut().languages.subtitle_index = 2;
    assert!(runtime.refresh_language(Subtitle).is_err());
    assert_eq!(runtime.host().languages.subtitle_index, u32::MAX);
    runtime.host_mut().languages.primary_index = 2;
    runtime.host_mut().languages.message_index = 2;
    runtime.refresh_language(Interface).unwrap();
    runtime.refresh_language(MainText).unwrap();
}
