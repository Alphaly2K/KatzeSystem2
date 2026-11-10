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
