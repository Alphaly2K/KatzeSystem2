use katze_system2::audio::{SoundConfig, SoundGroups};

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

    let [bgm, voice] = &config.parts[..] else {
        panic!("expected two parts");
    };
    assert_eq!(bgm.name, b"BGM");
    assert_eq!(bgm.type_name, b"bgm");
    assert_eq!(bgm.file_prefix, b"BGM");
    assert_eq!(bgm.archive, b"bgm");
    assert_eq!(bgm.id, 0);
    assert_eq!(bgm.master_volume, 60);
    assert_eq!(bgm.volume, 80);
    assert_eq!(bgm.voice_volume, 60);
    assert_eq!(bgm.skip_volume, -1);
    assert_eq!(bgm.system_voice, -1);
    assert_eq!(bgm.play_enabled, 1);
    assert_eq!(bgm.mute, -1);
    assert!(bgm.children.is_empty());

    assert_eq!(voice.file_prefix, b"");
    assert_eq!(voice.archive, b"");
    assert_eq!(voice.volume, 90);
    let [child0, child1] = &voice.children[..] else {
        panic!("expected two children");
    };
    // 未给出的字段沿用父级；显式字段与零值各自保留。
    assert_eq!(child0.name, b"n0");
    assert_eq!(child0.file_prefix, b"a_sys_");
    assert_eq!(child0.archive, b"pcm_a");
    assert_eq!(child0.id, 0);
    assert_eq!(child0.system_voice, 1);
    assert_eq!(child0.volume, 90);
    assert_eq!(child0.master_volume, 100);
    assert_eq!(child0.voice_volume, -1);
    assert_eq!(child0.skip_volume, -1);
    assert_eq!(child0.play_enabled, 1);
    assert_eq!(child0.mute, -1);
    assert_eq!(child1.volume, 30);
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
    assert_eq!(groups.entries[0].key, 0);
    assert_eq!(groups.entries[0].id, 0);
    assert_eq!(groups.entries[0].label, b"bgm");
    assert_eq!(groups.entries[0].range, [0, 5]);
    assert_eq!(groups.entries[1].key, 0);
    assert_eq!(groups.entries[1].label, b"bgm2");
    assert_eq!(groups.entries[2].id, 3);
    assert_eq!(groups.entries[2].range, [24, 24]);
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
    assert_eq!(config.master_mute, -1);
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
        xml("<a><id>text</id><volume>-2</volume><mute>+3</mute></a>").as_bytes(),
    )
    .unwrap();
    assert_eq!(config.parts[0].id, 0);
    assert_eq!(config.parts[0].volume, -2);
    assert_eq!(config.parts[0].mute, 3);

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
