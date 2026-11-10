use katze_system2::{
    audio::{SoundConfig, SoundGroups},
    resource::{XmlDocument, XmlWriter},
};

const CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<document>
	<SOUNDGROUP>
		<group00>
			<comment>ＢＧＭ</comment>
			<id>0</id>
			<label>bgm</label>
			<min>0</min>
			<max>5</max>
		</group00>
		<group01>
			<id>0</id>
			<label>bgm2</label>
			<min>0</min>
			<max>5</max>
		</group01>
		<group02>
			<id>3</id>
			<label>sysvoice</label>
			<min>24</min>
			<max>24</max>
		</group02>
	</SOUNDGROUP>
	<MASTER>
		<volume>100</volume>
		<play>1</play>
	</MASTER>
	<PART>
		<bgm>
			<name>BGM</name>
			<id>0</id>
			<type>bgm</type>
			<head>BGM</head>
			<group>0</group>
			<int>bgm</int>
			<mastervol>60</mastervol>
			<volume>80</volume>
			<play>1</play>
			<sysplay>-1</sysplay>
			<voicevol>60</voicevol>
		</bgm>
		<voice>
			<name>VOICE</name>
			<id>2</id>
			<sameid>7</sameid>
			<type>voice</type>
			<head></head>
			<group>2</group>
			<int></int>
			<mastervol>100</mastervol>
			<volume>90</volume>
			<play>1</play>
			<sysplay>-1</sysplay>
			<part>
				<V0>
					<name>n0</name>
					<id>0</id>
					<type>w</type>
					<head>a_sys_</head>
					<int>pcm_a</int>
					<play>1</play>
					<sysplay>1</sysplay>
				</V0>
				<V1>
					<name>n1</name>
					<id>1</id>
					<sameid>-1</sameid>
					<type>w</type>
					<head>b_</head>
					<int>pcm_b</int>
					<play>0</play>
					<sysplay>-1</sysplay>
					<volume>30</volume>
					<mute>0</mute>
				</V1>
			</part>
		</voice>
	</PART>
</document>"#;

#[test]
fn soundconf_restores_master_groups_tree_and_child_inheritance() {
    let config = SoundConfig::from_xml(CONFIG.as_bytes()).unwrap();
    assert_eq!(config.master_volume, Some(100));
    assert_eq!(config.master_play_enabled, Some(1));
    assert_eq!(config.master_system_voice, None);
    assert_eq!(config.master_mute(), 0);

    let [voice, bgm] = &config.parts[..] else {
        panic!("expected two parts");
    };
    assert_eq!(bgm.xml_key, b"bgm");
    assert_eq!(bgm.name, b"BGM");
    assert_eq!(bgm.type_name, b"bgm");
    assert_eq!(bgm.file_prefix, b"BGM");
    assert_eq!(bgm.archive, b"bgm");
    assert_eq!(bgm.id, 0);
    assert_eq!(bgm.same_id, 0);
    assert_eq!(bgm.master_volume, 60);
    assert_eq!(bgm.volume, 80);
    assert_eq!(bgm.voice_volume, 60);
    assert_eq!(bgm.skip_volume, -1);
    assert_eq!(bgm.system_voice, -1);
    assert_eq!(bgm.play_enabled, 1);
    assert_eq!(bgm.mute, 0);
    assert_eq!(bgm.voice_mute, 0);
    assert!(bgm.children.is_empty());

    assert_eq!(voice.file_prefix, b"");
    assert_eq!(voice.archive, b"");
    assert_eq!(voice.volume, 90);
    assert_eq!(voice.same_id, 7);
    let [child1, child0] = &voice.children[..] else {
        panic!("expected two children");
    };
    // 未给出的字段沿用父级；显式字段与零值各自保留。
    assert_eq!(child0.xml_key, b"v0");
    assert_eq!(child0.name, b"n0");
    assert_eq!(child0.file_prefix, b"a_sys_");
    assert_eq!(child0.archive, b"pcm_a");
    assert_eq!(child0.id, 0);
    assert_eq!(child0.same_id, 0);
    assert_eq!(child0.system_voice, 1);
    assert_eq!(child0.volume, 90);
    assert_eq!(child0.master_volume, 100);
    assert_eq!(child0.voice_volume, -1);
    assert_eq!(child0.skip_volume, -1);
    assert_eq!(child0.play_enabled, 1);
    assert_eq!(child0.mute, 0);
    assert_eq!(child1.volume, 30);
    assert_eq!(child1.same_id, -1);
    assert_eq!(child1.mute, 0);
    assert_eq!(child1.play_enabled, 0);

    assert_eq!(config.file_part(b"a_sys_009").unwrap().unwrap().id, 0);
    assert_eq!(config.file_part(b"BGM04").unwrap().unwrap().volume, 80);
    assert_eq!(config.volume(&[2, 0]).unwrap(), 90);
    assert_eq!(config.system_voice_candidates(&[]).unwrap(), [0]);
}

#[test]
fn soundconf_group_entries_keep_table_key_id_and_ranges() {
    let groups = SoundGroups::from_xml(CONFIG.as_bytes()).unwrap();
    assert_eq!(groups.entries.len(), 3);
    assert_eq!(groups.entries[2].key, 0);
    assert_eq!(groups.entries[2].id, 0);
    assert_eq!(groups.entries[2].label, b"bgm");
    assert_eq!(groups.entries[2].range, [0, 5]);
    assert_eq!(groups.entries[1].key, 0);
    assert_eq!(groups.entries[1].label, b"bgm2");
    assert_eq!(groups.entries[0].id, 3);
    assert_eq!(groups.entries[0].range, [24, 24]);
    assert_eq!(groups.id(b"bgm2"), 0);
    assert_eq!(groups.id(b"sysvoice"), 3);
    assert_eq!(groups.id(b"missing"), 0);
    assert_eq!(groups.id(b"bgm\0tail"), 0);
}

#[test]
fn soundconf_missing_sections_keep_unset_defaults() {
    let config =
        SoundConfig::from_xml(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document/>").unwrap();
    assert!(config.parts.is_empty());
    assert_eq!(config.master_volume, None);
    assert_eq!(config.master_mute, 0);
    let groups = SoundGroups::from_xml(b"<document/>").unwrap();
    assert!(groups.entries.is_empty());
}

#[test]
fn soundconf_integer_and_depth_rules_hold() {
    let xml = |inner: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><PART>{inner}</PART></document>"
        )
    };
    let config = SoundConfig::from_xml(
        xml("<a><id>text</id><volume>  -2</volume><mute>+3</mute><voicevol></voicevol><skipvol>1 2</skipvol></a>")
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(config.parts[0].id, 0);
    assert_eq!(config.parts[0].volume, -2);
    assert_eq!(config.parts[0].mute, 3);
    assert_eq!(config.parts[0].voice_volume, 0);
    assert_eq!(config.parts[0].skip_volume, 1);

    let mut deep = String::new();
    for _ in 0..40 {
        deep.push_str("<a><part>");
    }
    assert!(SoundConfig::from_xml(xml(&deep).as_bytes()).is_err());
    assert!(SoundConfig::from_xml(b"not xml").is_err());
    assert!(SoundConfig::from_xml(b"").is_err());
}

#[test]
fn soundconf_rejects_oversized_and_non_utf8_input() {
    // 读取上限为 64 MiB。
    let big = vec![b'x'; 64 * 1024 * 1024 + 1];
    assert!(SoundConfig::from_xml(&big).is_err());
    assert!(SoundConfig::from_xml(&[0xff, 0xfe, 0x00]).is_err());
}

#[test]
fn user_sound_settings_preserve_definition_defaults_and_persist_current_tree() {
    let definition = br#"<?xml version="1.0" encoding="UTF-8"?><document>
      <MASTER><volume>71</volume><play>3</play><sysplay>5</sysplay>
        <voicevol>17</voicevol><skipvol>19</skipvol><mute>23</mute><voicemute>29</voicemute></MASTER>
      <PART><alpha><name>Parent</name><id>7</id><volume>73</volume><play>11</play>
        <sysplay>13</sysplay><voicevol>31</voicevol><skipvol>37</skipvol><mute>41</mute><voicemute>43</voicemute>
        <part><CHILD><name>Child</name><id>17</id></CHILD>
          <explicit><id>19</id><mute>-1</mute><voicemute>0</voicemute></explicit></part>
      </alpha></PART></document>"#;
    let defaults = SoundConfig::from_xml(definition).unwrap();
    let alpha = &defaults.parts[0];
    assert_eq!((alpha.mute, alpha.voice_mute), (41, 43));
    assert_eq!(
        (alpha.children[0].mute, alpha.children[0].voice_mute),
        (-1, 0)
    );
    assert_eq!(
        (alpha.children[1].mute, alpha.children[1].voice_mute),
        (0, 0)
    );
    let current = SoundConfig::from_xml_with_user_settings(definition, None).unwrap();
    assert_eq!(current, defaults);
    let empty = br#"<?xml version="1.0" encoding="UTF-8"?><document/>"#;
    let current = SoundConfig::from_xml_with_user_settings(definition, Some(empty)).unwrap();
    assert_eq!((current.master_mute, current.master_voice_mute), (5, 5));
    assert_eq!(
        (current.parts[0].mute, current.parts[0].voice_mute),
        (13, 13)
    );
    let user = br#"<?xml version="1.0" encoding="UTF-8"?><document>
      <MASTER><volume>-2147483648</volume><sysplay>0</sysplay></MASTER>
      <PART><alpha><volume>200</volume><mute>-1</mute><id>99</id><name>Ignored</name>
        <part><child><volume>83</volume></child><unknown><volume>89</volume></unknown></part>
      </alpha><unknown><volume>97</volume></unknown></PART></document>"#;
    let config = SoundConfig::from_xml_with_user_settings(definition, Some(user)).unwrap();
    assert_eq!(config.master_volume, Some(i32::MIN));
    assert_eq!(config.master_system_voice, Some(0));
    assert_eq!((config.master_mute, config.master_voice_mute), (5, 5));
    let [alpha] = &config.parts[..] else {
        panic!("unexpected category")
    };
    assert_eq!(alpha.xml_key, b"alpha");
    assert_eq!((&alpha.name[..], alpha.id), (&b"Parent"[..], 7));
    assert_eq!((alpha.volume, alpha.mute, alpha.voice_mute), (200, -1, 13));
    let [explicit, child] = &alpha.children[..] else {
        panic!("unexpected children")
    };
    assert_eq!(
        (explicit.volume, explicit.mute, explicit.voice_mute),
        (73, 13, 13)
    );
    assert_eq!((child.volume, child.mute, child.voice_mute), (83, 13, 13));
    let mut writer = XmlWriter::new();
    config.write_xml(&mut writer).unwrap();
    let bytes = writer.to_bytes().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.find("<child>").unwrap() < text.find("<explicit>").unwrap());
    let xml = XmlDocument::parse(&bytes).unwrap();
    assert_eq!(xml.integer(b"MASTER/voicevol", 0).unwrap(), 17);
    assert_eq!(xml.integer(b"MASTER/skipvol", 0).unwrap(), 19);
    assert_eq!(xml.text(b"PART/alpha/name").unwrap(), None);
    assert_eq!(xml.text(b"PART/alpha/id").unwrap(), None);
    assert_eq!(
        SoundConfig::from_xml_with_user_settings(definition, Some(&bytes)).unwrap(),
        config
    );
}
