use katze_system2::resource::{XmlDocument, XmlWriter};

#[test]
fn writing_keeps_native_order_literal_text_and_container_value_rules() {
    let mut writer = XmlWriter::new();
    assert_eq!(
        writer.to_bytes().unwrap(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<document></document>\r\n"
    );
    writer.write(&[], b"root text").unwrap();
    writer.write(&["group"], b"parent text").unwrap();
    writer.write(&["group", "first"], b"old").unwrap();
    writer.write(&["group", "second"], b"second").unwrap();
    writer
        .write(&["group", "first"], b" \t&entity; <literal>\r\n")
        .unwrap();
    writer.write_integer(&["number"], 0x8000_0000).unwrap();
    writer.write(&["number\0tail"], b"-1\0tail").unwrap();
    writer.write(&["empty"], b"").unwrap();
    assert_eq!(writer.to_bytes().unwrap(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<document>\r\n  <empty></empty>\r\n  <number>-1</number>\r\n  <group>\r\n    <second>second</second>\r\n    <first> \t&entity; <literal>\r\n</first>\r\n  </group>\r\n</document>\r\n");
}

#[test]
fn writing_wide_text_preserves_unit_conversion_and_failed_replacements() {
    let mut writer = XmlWriter::new();
    writer.write_integer(&["signed"], 0x8000_0000).unwrap();
    writer
        .write_wide(&["wide"], &[0x41, 0x3a9, 0xd83d, 0xde00, 0xffff, 0, 0x42])
        .unwrap();
    let mut expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<document>\r\n  <wide>AΩ��"
        .as_bytes()
        .to_vec();
    expected.extend(b"\xff\xff</wide>\r\n  <signed>-2147483648</signed>\r\n</document>\r\n");
    assert_eq!(writer.to_bytes().unwrap(), expected);
    assert!(writer.write_wide(&["wide"], &vec![0x41; 4096]).is_err());
    assert_eq!(writer.to_bytes().unwrap(), expected);
    writer.write_wide(&["wide"], &vec![0x41; 4095]).unwrap();
}

fn xml(body: &str) -> XmlDocument {
    XmlDocument::parse(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}").as_bytes())
        .unwrap()
}

#[test]
fn fields_merge_duplicate_paths_and_keep_raw_text_and_literal_names() {
    let document = xml("<document><a>old<b>7</b></a><a>1<c>4</c>3<!--skip-->5</a>
         <v></v><ns:x><ns:y>資料&amp;&#65;</ns:y></ns:x>
         <n>one\r\ntwo\nthree\tfour\rfive</n></document><ignored>tail</ignored>");
    for path in [b"a".as_slice(), b"//a///", b"a\0ignored"] {
        assert_eq!(document.text(path).unwrap(), Some(b"135".as_slice()));
    }
    assert_eq!(document.integer(b"a/b", 99).unwrap(), 7);
    assert_eq!(document.integer(b"a/c", 99).unwrap(), 4);
    assert_eq!(document.integer(b"v", 99).unwrap(), 0);
    assert_eq!(document.integer(b"missing", 99).unwrap(), 99);
    assert_eq!(
        document.text(b"ns:x/ns:y").unwrap(),
        Some("資料&amp;&#65;".as_bytes())
    );
    assert_eq!(
        document.text(b"n").unwrap(),
        Some(b"one two three four\rfive".as_slice())
    );
    for path in [b"".as_slice(), b"document/a", b"A", b"a/./b", b"ignored"] {
        assert_eq!(document.text(path).unwrap(), None);
    }
    let other = xml("<root><a>+0x10tail</a><b>12.5</b></root>");
    assert_eq!(other.text(b"a").unwrap(), None);
    assert_eq!(other.integer(b"root/a", 99).unwrap(), 16);
    assert_eq!(other.integer(b"root/b", 99).unwrap(), 0);
    assert!(other.text(&vec![b'a'; 512]).is_err());
}

#[test]
fn parsing_requires_baseline_declaration_and_rejects_unsupported_structure() {
    let lower = b"<?xml version=\"1.0\" encoding=\"utf-8\"?><document/>";
    assert!(XmlDocument::parse(lower).is_ok());
    assert!(xml("<document/>").text(b"document").unwrap().is_none());
    for body in [
        "<document><a/></document>",
        "<document><![CDATA[text]]></document>",
        "<document><a></document>",
        "<document>",
    ] {
        let source = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
        assert!(XmlDocument::parse(source.as_bytes()).is_err());
    }
    for source in [
        b"<document></document>".as_slice(),
        b" <?xml version=\"1.0\" encoding=\"UTF-8\"?><document/>",
        b"<?xml version=\"1.1\" encoding=\"UTF-8\"?><document/>",
        b"<?xml version=\"1.0\" encoding=\"Shift_JIS\"?><document/>",
    ] {
        assert!(XmlDocument::parse(source).is_err());
    }
}
