use katze_system2::resource::{ArchiveConfig, Category};

fn config_xml(body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><document>{body}</document>")
}

#[test]
fn category_resolution_keeps_directory_bypass_and_baseline_cp932_boundaries() {
    let category = Category {
        prefix: b"mot".to_vec(),
        extension: b"kcs".to_vec(),
    };
    for (input, expected) in [
        (b"sample".as_slice(), b"mot/sample.kcs".as_slice()),
        (b"sample.KCS", b"mot/sample.KCS"),
        (b"sample.", b"mot/sample."),
        (b".sample", b"mot/.sample"),
        (b"folder/sample", b"folder/sample"),
        (b"folder\\sample", b"folder\\sample"),
        (b"/sample", b"mot//sample.kcs"),
        (b"C:sample", b"mot/C:sample.kcs"),
        (b"C:\\sample", b"C:\\sample"),
        (b"a\x81\x5cb", b"mot/a\x81\x5cb.kcs"),
        (b"a.\x81\x5cb", b"mot/a.\x81\x5cb.kcs"),
        (b"a\xf0\x5cb", b"a\xf0\x5cb"),
        (b"sample\0ignored", b"mot/sample.kcs"),
        (b"", b"mot/.kcs"),
    ] {
        assert_eq!(category.resolve(input).unwrap(), expected);
    }
    let category = Category {
        prefix: b"PFX/\0ignored".to_vec(),
        extension: b".kcs\0ignored".to_vec(),
    };
    assert_eq!(category.resolve(b"sample").unwrap(), b"PFX//sample..kcs");
    assert_eq!(Category::default().resolve(b"sample").unwrap(), b"sample.");
}

#[test]
fn archive_config_merges_paths_and_resolves_labels_using_archive_prefixes() {
    let config = ArchiveConfig::from_xml(&config_xml(
        "<INT>
          <int00><name>mot</name><int>archive</int><ext>old</ext><dir>loose</dir></int00>
          <int01><name>MOT</name><int>second</int></int01>
          <int127><name>last</name><int>tail</int></int127>
          <int128><name>ignored</name><int>outside</int></int128>
          <need00>0</need00><need63>127</need63><need64>3</need64>
         </INT>
         <INT><int00><ext>kcs</ext></int00><need00>1</need00></INT>",
    ))
    .unwrap();
    assert_eq!(config.find(b"MoT\0ignored"), Some(0));
    assert_eq!(config.find(b""), Some(2));
    assert_eq!(config.find(b"last"), Some(127));
    assert_eq!(config.find(b"ignored"), None);
    assert_eq!(config.entries[0].directory, b"loose");
    assert_eq!(
        config.resolve(b"MOT", b"sample").unwrap(),
        b"archive/sample.kcs"
    );
    assert_eq!(config.required[0], 1);
    assert_eq!(config.required[1], -1);
    assert_eq!(config.required[63], 127);
    assert!(config.resolve(b"unknown", b"sample").is_err());
    assert_eq!(
        config.resolve(b"unknown", b"folder/sample").unwrap(),
        b"folder/sample"
    );
    assert!(config.resolve(b"unknown", b"/sample").is_err());
}

#[test]
fn archive_config_retains_literal_entities_direct_text_and_numeric_prefixes() {
    let config = ArchiveConfig::from_xml(&config_xml(
        "<INT><int00><name>mot</name><int>a&amp;b</int><ext>kcs</ext>
         <dir>資料\r\n a\tb\rc<!--ignored--><nested>ignored</nested>d</dir></int00>
         <int00><name></name></int00>
         <need00> 7 </need00><need01>-0x10</need01><need02>12tail</need02>
         <need03>1.5</need03><need04>4294967296</need04>
         <need05>2147483648</need05><need06>010</need06></INT>",
    ))
    .unwrap();
    assert!(config.entries[0].name.is_empty());
    assert_eq!(config.entries[0].category.prefix, b"a&amp;b");
    assert_eq!(config.entries[0].directory, "資料  a b\rcd".as_bytes());
    assert_eq!(&config.required[..7], &[0, -16, 12, 0, 0, i32::MIN, 10]);
}

#[test]
fn archive_config_checks_baseline_xml_forms_and_fixed_field_capacities() {
    for input in [
        "<document/>",
        &format!("\u{feff}{}", config_xml("")),
        &config_xml("<INT><int00><name/></int00></INT>"),
        &config_xml("<INT><int00><name><![CDATA[mot]]></name></int00></INT>"),
        &config_xml("<INT><int00><ext>12345678</ext></int00></INT>"),
        &config_xml("<INT>"),
    ] {
        assert!(ArchiveConfig::from_xml(input).is_err());
    }
    let config = ArchiveConfig::from_xml(&config_xml(
        "<int><int00><name>wrong</name></int00></int>
         <INT xmlns:p=\"example\"><p:int00><name>wrong</name></p:int00></INT>",
    ))
    .unwrap();
    assert_eq!(config, ArchiveConfig::default());
}
