use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    resource::{XmlDocument, XmlWriter},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};

fn settings() -> SystemSettings {
    SystemSettings {
        sound: [0x11; 0x15fc],
        message: [0x22; 0x48],
        effects: [0x33; 0x20],
        window: [0x44; 0x0c],
        auto_select: 0x80000007,
        scene_animation_enabled: 0x8000_0005,
        fullscreen: 0x8000_0000,
        fullscreen_mw_pos: 0x8000_0003,
        ..SystemSettings::default()
    }
}

fn arguments(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

type ConfigurationWrites = std::rc::Rc<
    std::cell::RefCell<Vec<(u32, katze_system2::settings::ConfigurationFile, Vec<u8>)>>,
>;

struct ConfigurationSink(ConfigurationWrites);

impl katze_system2::settings::ConfigurationServices for ConfigurationSink {
    fn write(
        &mut self,
        task: u32,
        file: katze_system2::settings::ConfigurationFile,
        bytes: &[u8],
    ) -> Result<bool> {
        self.0.borrow_mut().push((task, file, bytes.to_vec()));
        Ok(false)
    }
}

#[test]
fn ordinary_configuration_save_keeps_file_failures_local_and_sound_gate_independent() {
    use katze_system2::{
        audio::SoundConfig,
        input::DeviceKeys,
        settings::{ConfigurationFile, PlatformSettings},
    };
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0xaa; 32], 16).unwrap(), vec![]).unwrap();
    assert!(
        host.call(361, &[], &mut vm)
            .unwrap_err()
            .message
            .contains("写入服务")
    );
    let writes = ConfigurationWrites::default();
    host.configuration_services = Some(Box::new(ConfigurationSink(writes.clone())));
    host.system_settings = Some(SystemSettings::default());
    host.platform_settings = Some(PlatformSettings {
        sound_stereo: 0,
        ..PlatformSettings::default()
    });
    host.configuration_keys = Some(DeviceKeys::default());
    host.sound_config = Some(SoundConfig {
        master_volume: Some(37),
        ..SoundConfig::default()
    });
    host.task_id = 71;
    host.system_settings_dirty = 9;
    host.sound_settings_dirty = 17;
    host.system_settings.as_mut().unwrap().jtalk_enabled = 1;
    assert_eq!(Host::sound_stereo(&host), 0);
    host.sound_stereo = Some(7);
    assert_eq!(Host::sound_stereo(&host), 7);
    assert_eq!(
        host.call(361, &[], &mut vm).unwrap(),
        NativeReturn::Continue
    );
    host.system_settings.as_mut().unwrap().jtalk_enabled = 0;
    host.sound_config_enabled = 0x8000_0000;
    assert_eq!(
        host.call(361, &[], &mut vm).unwrap(),
        NativeReturn::Continue
    );
    assert!(host.call(361, &[0; 4], &mut vm).is_err());
    assert_eq!(vm.read(0, 32).unwrap(), vec![0xaa; 32]);
    assert_eq!(
        [host.system_settings_dirty, host.sound_settings_dirty],
        [9, 17]
    );
    let writes = writes.borrow();
    assert_eq!(
        writes
            .iter()
            .map(|(task, file, _)| (*task, *file))
            .collect::<Vec<_>>(),
        [
            (71, ConfigurationFile::Main),
            (71, ConfigurationFile::Main),
            (71, ConfigurationFile::Sound)
        ]
    );
    assert_eq!(
        XmlDocument::parse(&writes[2].2)
            .unwrap()
            .integer(b"MASTER/volume", 99)
            .unwrap(),
        37
    );
}

#[test]
fn platform_overlay_checks_fullscreen_candidates_before_truncation_and_keeps_inheritance() {
    use katze_system2::settings::PlatformSettings;
    let mut platform = PlatformSettings {
        fullscreen_size: [1920, 1080],
        etc_disablemovie: 7,
        directories: [Some(vec![65]), None, Some(vec![66])],
        tts_parameters: [0xffc0_1234, 0x3f80_0000],
        ..PlatformSettings::default()
    };
    let input = br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <WINDOW><width>65537</width><height>480</height><full_width>66816</full_width><full_height>720</full_height><full_wide>99</full_wide></WINDOW>
        <ETC><cache>99</cache><export></export></ETC>
        <DEPEND><z32>99</z32><use_z>99</use_z></DEPEND>
        <TTS><tone>-0</tone></TTS><CONFIG><type>-1</type></CONFIG><KEY><enable_joypad>257</enable_joypad></KEY>
        </document>"#;
    let mut candidates = Vec::new();
    platform
        .apply_user_xml(input, |size| {
            candidates.push(size);
            size == [1280, 720]
        })
        .unwrap();
    assert_eq!(candidates, [[66816, 720]]);
    assert_eq!(platform.window_size, [1, 480]);
    assert_eq!(platform.fullscreen_size, [1920, 1080]);
    assert_eq!(
        [
            platform.window_full_wide,
            platform.etc_cache,
            platform.depend_z32,
            platform.depend_use_z
        ],
        [0, 1, 0, 1]
    );
    assert_eq!(platform.etc_disablemovie, 0);
    assert_eq!(
        platform.directories,
        [Some(vec![65]), Some(vec![]), Some(vec![66])]
    );
    assert_eq!(platform.tts_parameters, [0xffc0_1234, 0x8000_0000]);
    assert_eq!(
        [platform.config_type, platform.joypad_enabled],
        [u32::MAX, 257]
    );
    let empty = br#"<?xml version="1.0" encoding="UTF-8"?><document/>"#;
    platform
        .apply_user_xml(empty, |size| {
            assert_eq!(size, [1, 480]);
            true
        })
        .unwrap();
    assert_eq!(platform.fullscreen_size, [1, 480]);

    let startup = br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <SCREEN><width>65537</width><height>480</height><full_width>66816</full_width><full_height>66256</full_height><fps>75</fps></SCREEN>
        <WINDOW><fps>99</fps></WINDOW><TEXTURE><filter_min>-1</filter_min><min>99</min></TEXTURE>
        <SOUND><stereo>99</stereo><use3d>7</use3d></SOUND></document>"#;
    platform.etc_cache = 17;
    platform
        .apply_startup_xml(startup, |candidate| {
            assert_eq!(candidate, [1280, 720]);
            [1920, 1080]
        })
        .unwrap();
    assert_eq!(platform.window_size, [1, 480]);
    assert_eq!(platform.fullscreen_size, [1920, 1080]);
    assert_eq!(
        [
            platform.window_fps,
            platform.texture_min,
            platform.sound_ds3d,
            platform.sound_stereo,
            platform.etc_cache
        ],
        [75, u32::MAX, 7, 1, 1]
    );
}

#[test]
fn complete_configuration_merges_shared_window_fields_and_places_keys_with_user_options() {
    use katze_system2::{
        input::{DeviceBinding, DeviceKeys},
        language::LanguageState,
        settings::{PlatformSettings, write_configuration},
    };
    let system = SystemSettings {
        fullscreen: 7,
        fullscreen_mw_pos: u32::MAX,
        font_names: [Some(vec![65]), Some(vec![66])],
        ..SystemSettings::default()
    };
    let platform = PlatformSettings {
        directories: [Some(vec![67]), None, Some(vec![])],
        tts_parameters: [0x3eaa_aaab, 0x8000_0000],
        config_type: 0x8000_0000,
        joypad_enabled: 17,
        ..PlatformSettings::default()
    };
    let mut bindings = [DeviceBinding::default(); 80];
    bindings[3].enabled = 1;
    bindings[3].codes[0][0] = 65;
    let keys = DeviceKeys::new(bindings);
    let languages = LanguageState {
        primary_index: 3,
        message_index: 2,
        ..LanguageState::default()
    };
    let mut writer = XmlWriter::new();
    write_configuration(&mut writer, &system, &platform, &languages, &keys).unwrap();
    let bytes = writer.to_bytes().unwrap();
    let source = std::str::from_utf8(&bytes).unwrap();
    for (before, after) in [
        ("<KEY>", "<FONT>"),
        ("<FONT>", "<CONFIG>"),
        ("<CONFIG>", "<TTS>"),
        ("<TTS>", "<language>"),
    ] {
        assert!(source.find(before).unwrap() < source.find(after).unwrap());
    }
    assert_eq!(source.matches("<fullscreen>").count(), 1);
    let xml = XmlDocument::parse(&bytes).unwrap();
    for (path, value) in [
        (b"WINDOW/fullscreen".as_slice(), 7),
        (b"FULL_SCREEN/mw_pos", u32::MAX),
        (b"CONFIG/type", 0x8000_0000),
        (b"KEY/enable_joypad", 17),
        (b"KEY/kb1_03", 65),
        (b"language/id/UI", 3),
        (b"language/id/main", 2),
    ] {
        assert_eq!(xml.integer(path, 99).unwrap(), value);
    }
    assert!(xml.text(b"ETC/export").unwrap().is_none());
    assert!(xml.text(b"KEY/kb1_02").unwrap().is_none());
    assert_eq!(
        xml.text(b"TTS/speed").unwrap(),
        Some(b"0.333333".as_slice())
    );
    assert_eq!(
        xml.text(b"TTS/tone").unwrap(),
        Some(b"-0.000000".as_slice())
    );
}

#[test]
fn user_settings_round_trip_raw_fields_and_keep_startup_data_and_inherited_fonts() {
    let mut source = settings();
    let mut value = 1u32;
    for block in [
        &mut source.sound[..],
        &mut source.message[..],
        &mut source.effects[..],
        &mut source.window[..],
    ] {
        for bytes in block.chunks_exact_mut(4) {
            bytes.copy_from_slice(&(value ^ 0x8000_0000).to_le_bytes());
            value += 17;
        }
    }
    source.font_controls = [7, 9, 3, u32::MAX, 17];
    source.font_names = [
        Some("Primary & Demo Ω".encode_utf16().collect()),
        Some("Ruby 資料".encode_utf16().collect()),
    ];
    source.system_voice_play = std::array::from_fn(|i| 0x8000_0000 + i as u32);
    source.always_active = 9;
    source.scene_skip_enabled = 33;
    source.jtalk_enabled = u32::MAX;
    source.texture_size = [16, 32];
    source.auto_wait_presets[4] = [777, 888];
    source.local_flag_ranges[2] = [11, 19];
    let mut writer = XmlWriter::new();
    source.write_xml(&mut writer).unwrap();
    let bytes = writer.to_bytes().unwrap();
    let voice = std::str::from_utf8(&bytes)
        .unwrap()
        .split_once("<VOICE>")
        .unwrap()
        .1
        .split_once("</VOICE>")
        .unwrap()
        .0
        .as_bytes();
    let position = |name: &[u8]| {
        voice
            .windows(name.len())
            .position(|part| part == name)
            .unwrap()
    };
    assert!(position(b"<cont_voice>") < position(b"<play47>"));
    assert!(position(b"<play00>") < position(b"<vol_master>"));
    let baseline = SystemSettings::default();
    let mut restored = baseline.clone();
    restored.apply_xml(&bytes).unwrap();
    let mut game_defaults = baseline.clone();
    game_defaults.apply_game_xml(&bytes).unwrap();
    let mut expected_defaults = restored.clone();
    expected_defaults.font_names = baseline.font_names.clone();
    expected_defaults.font_controls = baseline.font_controls;
    expected_defaults.fullscreen = baseline.fullscreen;
    expected_defaults.fullscreen_mw_pos = baseline.fullscreen_mw_pos;
    assert_eq!(game_defaults, expected_defaults);
    assert_eq!(restored.message, source.message);
    assert_eq!(restored.effects, source.effects);
    assert_eq!(restored.window, source.window);
    assert_eq!(restored.font_names, source.font_names);
    assert_eq!(restored.font_controls, source.font_controls);
    assert_eq!(restored.system_voice_play, source.system_voice_play);
    assert_eq!(
        [
            restored.auto_select,
            restored.always_active,
            restored.scene_animation_enabled,
            restored.scene_skip_enabled,
            restored.jtalk_enabled,
            restored.fullscreen,
            restored.fullscreen_mw_pos
        ],
        [
            source.auto_select,
            source.always_active,
            source.scene_animation_enabled,
            source.scene_skip_enabled,
            source.jtalk_enabled,
            source.fullscreen,
            source.fullscreen_mw_pos
        ]
    );
    let word = |block: &[u8], offset: usize| {
        u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap())
    };
    for offset in std::iter::once(16)
        .chain((0..48).map(|i| 20 + i * 116))
        .chain((0x15d4..0x15fc).step_by(4))
    {
        assert_eq!(word(&restored.sound, offset), word(&source.sound, offset));
    }
    assert_eq!(&restored.sound[..16], &baseline.sound[..16]);
    assert_eq!(restored.texture_size, baseline.texture_size);
    assert_eq!(restored.auto_wait_presets, baseline.auto_wait_presets);
    assert_eq!(restored.local_flag_ranges, baseline.local_flag_ranges);
    let mut writer = XmlWriter::new();
    baseline.write_xml(&mut writer).unwrap();
    let xml = XmlDocument::parse(&writer.to_bytes().unwrap()).unwrap();
    assert_eq!(xml.text(b"FONT/face").unwrap(), None);
    assert_eq!(xml.text(b"FONT/faceruby").unwrap(), None);
}

#[test]
fn defaults_and_xml_overlays_keep_missing_values_raw_flags_and_volume_update_order() {
    fn word(block: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap())
    }
    let mut state = SystemSettings::default();
    assert_eq!(state.auto_select, 0);
    assert_eq!(state.scene_animation_enabled, 1);
    assert_eq!(state.system_voice_play, [1; 48]);
    assert_eq!(state.font_controls, [1, 1, 0, 1, 0]);
    let mut defaults = katze_system2::message::MessageDefaults::default();
    for (controls, expected) in [
        ([1, 1, 0, 1, 0], [400, 1, 1, 0]),
        ([7, 9, 0x8000_0000, 0, 23], [700, 9, 7, 1]),
        ([3, 5, 1, 9, u32::MAX], [700, 5, 3, u32::MAX]),
    ] {
        let font_settings = SystemSettings {
            font_controls: controls,
            ..SystemSettings::default()
        };
        font_settings.apply_message_defaults(&mut defaults);
        let configuration = defaults
            .configuration(katze_system2::message::TextEncoding::Cp932)
            .unwrap();
        assert_eq!(
            [0xc4, 0xc8, 0xcc, 0xd0].map(|offset| word(&configuration, offset)),
            expected
        );
    }
    assert_eq!(state.always_active, 1);
    assert_eq!((state.scene_skip_enabled, state.jtalk_enabled), (0, 0));
    assert_eq!(state.auto_wait_presets[4], [15, 5]);
    assert_eq!(state.auto_wait_presets[10..], [[0; 2]; 22]);
    assert_eq!((state.fullscreen, state.fullscreen_mw_pos), (0, 0));
    assert_eq!(state.texture_size, [256; 2]);
    assert_eq!(state.texture_grid, [0; 2]);
    assert_eq!(state.texture_areas, [[u32::MAX; 4]; 8]);
    assert_eq!((state.texture_32bit_row, state.texture_16bit_row), (0, 0));
    state
        .apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><GAME><autoselect>0x80000000</autoselect></GAME><SCREEN><mw_pos>17</mw_pos></SCREEN><WINDOW><fullscreen>1</fullscreen></WINDOW></document>"#)
        .unwrap();
    assert_eq!(state.auto_select, 0x80000000);
    assert_eq!((state.fullscreen, state.fullscreen_mw_pos), (0, 17));
    assert_eq!(word(&state.message, 12), 15);
    assert_eq!(word(&state.message, 16), 5);
    assert_eq!(word(&state.message, 44), u32::MAX);
    assert_eq!(word(&state.effects, 20), 5);
    assert_eq!(word(&state.window, 0), 0x64ffffff);
    assert_eq!(&state.sound[..16], [0; 16]);
    assert_eq!(word(&state.sound, 20), 1);
    assert_eq!(word(&state.sound, 20 + 47 * 116 + 80), 100);
    state
        .apply_xml(
            br#"<?xml version="1.0" encoding="UTF-8"?><document>
                <VOICE><vol_master>65536</vol_master><vol_bgm>65537</vol_bgm>
                    <vol_play>invalid</vol_play><cont_voice>-7</cont_voice>
                    <play47>0x80000000</play47><play48>9</play48><mute_master>2</mute_master>
                </VOICE>
                <MESSAGE><speed>9</speed><CHAR>0x22tail</CHAR><char>99</char></MESSAGE>
                <MESSAGE><speed>3</speed></MESSAGE>
                <GAME><wndcolor>0x80112233</wndcolor><enabledrag>-1</enabledrag><autoselect>-7</autoselect><enable_scene_anm>-3tail</enable_scene_anm><always_active>-2</always_active><use_scene_skip>0x80000001</use_scene_skip><use_jtalk>invalid</use_jtalk></GAME>
                <SYSTEMVOICE><play00>0</play00><play47>-7</play47><play48>9</play48></SYSTEMVOICE>
                <EFFECT><auto_cds>11</auto_cds><skipmot></skipmot></EFFECT>
                <WINDOW><fullscreen>-3</fullscreen></WINDOW>
                <FULL_SCREEN><mw_pos>0x80000001</mw_pos></FULL_SCREEN>
                <SCREEN><mw_pos>99</mw_pos></SCREEN>
            </document>"#,
        )
        .unwrap();
    assert_eq!(&state.sound[..16], [0; 16]);
    assert_eq!(word(&state.sound, 0x10), (-7i32) as u32);
    assert_eq!(word(&state.sound, 20 + 47 * 116), 0x80000000);
    assert_eq!(word(&state.sound, 0x15d4), 65536);
    assert_eq!(word(&state.sound, 0x15e8), 2);
    assert_eq!(word(&state.message, 0), 3);
    assert_eq!(word(&state.message, 16), 0x22);
    assert_eq!(word(&state.effects, 20), 11);
    assert_eq!(word(&state.effects, 28), 0);
    assert_eq!(word(&state.window, 0), 0x80112233);
    assert_eq!(word(&state.window, 4), u32::MAX);
    assert_eq!(state.auto_select, (-7i32) as u32);
    assert_eq!(state.scene_animation_enabled, (-3i32) as u32);
    assert_eq!(state.always_active, (-2i32) as u32);
    assert_eq!(state.scene_skip_enabled, 0x80000001);
    assert_eq!(state.jtalk_enabled, 0);
    assert_eq!(state.system_voice_play[0], 0);
    assert_eq!(state.system_voice_play[1..47], [1; 46]);
    assert_eq!(state.system_voice_play[47], (-7i32) as u32);
    assert_eq!(
        (state.fullscreen, state.fullscreen_mw_pos),
        ((-3i32) as u32, 0x8000_0001)
    );
    state.refresh_sound_volumes();
    assert_eq!(word(&state.sound, 0), 655);
    assert_eq!(word(&state.sound, 4), 65536);
    assert_eq!(word(&state.sound, 8), 65536);
    assert_eq!(word(&state.sound, 12), 0);
    let snapshot = state.clone();
    state
        .apply_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><MESSAGE><speed>3</speed></MESSAGE></document>"#)
        .unwrap();
    assert_eq!(state, snapshot);
    assert!(state.apply_xml(b"<document>").is_err());
    assert_eq!(state, snapshot);

    assert_eq!(state.local_flag_ranges[0], [0, 512]);
    assert_eq!(state.global_flag_ranges[0], [512, 512]);
    assert_eq!(state.local_flag_ranges[1..], [[u32::MAX; 2]; 7]);
    assert_eq!(state.global_flag_ranges[1..], [[u32::MAX; 2]; 7]);
    state.apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><TEXTURE><size><x>128</x></size><vt><x>4</x><y>7</y></vt><area07><sx>2</sx><sy>3</sy><dx>2</dx><dy>4</dy></area07><bpp32y>1</bpp32y><bpp16y>3</bpp16y></TEXTURE></document>"#).unwrap();
    assert_eq!(state.texture_size, [128, 256]);
    assert_eq!(state.texture_grid, [4, 7]);
    assert_eq!(state.texture_areas[7], [2, 3, 2, 4]);
    assert_eq!(state.texture_areas[..7], [[u32::MAX; 4]; 7]);
    assert_eq!((state.texture_32bit_row, state.texture_16bit_row), (1, 3));
    state
        .apply_startup_xml(
            br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <GAME><flag>
            <local0><start>9</start></local0>
            <global7><start>0x80000000</start><count>-2tail</count></global7>
            <local8><start>4</start></local8>
        </flag></GAME></document>"#,
        )
        .unwrap();
    assert_eq!(state.local_flag_ranges[0], [9, 512]);
    assert_eq!(state.local_flag_ranges[1..], [[u32::MAX; 2]; 7]);
    assert_eq!(state.global_flag_ranges[7], [0x80000000, (-2i32) as u32]);
    assert_eq!(state.sound, snapshot.sound);
    let mut expected_message = snapshot.message;
    expected_message[12..16].copy_from_slice(&15u32.to_le_bytes());
    expected_message[16..20].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(state.message, expected_message);
    assert_eq!(state.auto_select, snapshot.auto_select);
    state
        .apply_startup_xml(
            br#"<?xml version="1.0" encoding="UTF-8"?><document>
        <GAME><flag><local0><count>invalid</count></local0></flag><autoselect>4294967295</autoselect></GAME>
        </document>"#,
        )
        .unwrap();
    assert_eq!(state.local_flag_ranges[0], [9, 0]);
    assert_eq!(state.auto_select, u32::MAX);
    state.apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><GAME><wndcolor><r>17</r><g>34</g><b>51</b><a>204</a></wndcolor></GAME></document>"#).unwrap();
    assert_eq!(word(&state.window, 0), 0x50112233);
    state.apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><CONFIG_B><wnd_alpha>0</wnd_alpha></CONFIG_B><GAME><wndcolor><a>73</a></wndcolor></GAME></document>"#).unwrap();
    assert_eq!(word(&state.window, 0), 0x49112233);
    let snapshot = state.clone();
    assert!(state.apply_startup_xml(b"<document>").is_err());
    assert_eq!(state, snapshot);
}

#[test]
fn font_xml_overlays_keep_independent_names_and_default_fixed_to_new_proportional() {
    let mut settings = SystemSettings {
        font_names: [Some(vec![80]), Some(vec![82])],
        font_controls: [11, 12, 13, 14, 15],
        ..SystemSettings::default()
    };
    settings
        .apply_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document/>"#)
        .unwrap();
    assert_eq!(settings.font_controls, [11, 12, 13, 14, 14]);
    assert_eq!(settings.font_names, [Some(vec![80]), Some(vec![82])]);
    settings
        .apply_xml(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><FONT>\
        <face>あい Face</face><faceruby>\u{40000}</faceruby>\
        <shadow><around>-7</around><rightdown>9</rightdown></shadow>\
        <bold>0x80000000</bold><proportional>-3</proportional></FONT></document>"
                .as_bytes(),
        )
        .unwrap();
    assert_eq!(
        settings.font_names[0],
        Some("あい Face".encode_utf16().collect())
    );
    assert_eq!(settings.font_names[1], Some(vec![0xd8c0]));
    assert_eq!(
        settings.font_controls,
        [
            (-7i32) as u32,
            9,
            0x8000_0000,
            (-3i32) as u32,
            (-3i32) as u32
        ]
    );
    let mut defaults = katze_system2::message::MessageDefaults::default();
    settings.apply_message_defaults(&mut defaults);
    assert_eq!(defaults.font_name, settings.font_names[0].clone().unwrap());
    assert_eq!(defaults.outline, (-7i32) as u32);
    assert_eq!(defaults.shadow, 9);
    assert!(defaults.bold);
    assert_eq!(defaults.glyph_parameter, Some((-3i32) as u32));

    settings.apply_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><FONT><face>A&amp;B</face><proportional>0</proportional><fixed>37</fixed></FONT><FONT><face></face></FONT><font><faceruby>ignored</faceruby></font></document>"#).unwrap();
    assert_eq!(settings.font_names, [Some(vec![]), Some(vec![0xd8c0])]);
    assert_eq!(&settings.font_controls[3..], [0, 37]);
    settings.apply_message_defaults(&mut defaults);
    assert!(defaults.font_name.is_empty());
    assert_eq!(defaults.glyph_parameter, Some(1));
    settings.apply_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><FONT><bold>invalid</bold></FONT></document>"#).unwrap();
    assert_eq!(&settings.font_controls[2..], [0, 0, 0]);
    settings.apply_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><FONT><face>A&amp;B</face></FONT></document>"#).unwrap();
    assert_eq!(
        settings.font_names[0],
        Some("A&amp;B".encode_utf16().collect())
    );
}

#[test]
fn font_xml_capacity_failures_preserve_the_entire_previous_configuration() {
    let mut settings = SystemSettings::default();
    for length in [63, 64, 125, 126] {
        let snapshot = settings.clone();
        let input = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><GAME><autoselect>9</autoselect></GAME><FONT><face>Changed</face><faceruby>{}</faceruby><bold>1</bold></FONT></document>",
            "あ".repeat(length)
        );
        let result = settings.apply_xml(input.as_bytes());
        if length == 63 {
            result.unwrap();
            assert_eq!(settings.font_names[1], Some(vec![0x3042; 63]));
            assert_eq!(
                settings.font_names[0],
                Some("Changed".encode_utf16().collect())
            );
        } else {
            assert!(result.unwrap_err().message.contains("63"));
            assert_eq!(settings, snapshot);
        }
    }
}

#[test]
fn startup_auto_wait_presets_keep_raw_pairs_and_reselect_the_current_speed() {
    let mut settings = SystemSettings::default();
    settings.message[4..8].copy_from_slice(&31u32.to_le_bytes());
    settings.apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document><GAME><autopriset><p31><base>0x80000000</base><char>-2</char></p31><p04><base>7</base></p04><p32><base>99</base></p32></autopriset></GAME></document>"#).unwrap();
    assert_eq!(settings.auto_wait_presets[31], [0x80000000, (-2i32) as u32]);
    assert_eq!(settings.auto_wait_presets[4], [7, 5]);
    assert_eq!(&settings.message[12..16], 0x80000000u32.to_le_bytes());
    assert_eq!(&settings.message[16..20], (-2i32).to_le_bytes());
    settings
        .apply_startup_xml(br#"<?xml version="1.0" encoding="UTF-8"?><document/>"#)
        .unwrap();
    assert_eq!(settings.auto_wait_presets[31], [0x80000000, (-2i32) as u32]);
}

#[test]
fn settings_reads_keep_zero_addresses_overlap_order_and_partial_failures() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xaa; 6000], 0).unwrap(),
        vec![0xbb; 16],
    )
    .unwrap();
    let nulls = arguments(&[u32::MAX; 4]);
    assert_eq!(
        host.call(359, &nulls, &mut vm).unwrap(),
        NativeReturn::Continue
    );
    assert!(host.call(359, &arguments(&[0; 4]), &mut vm).is_err());
    assert!(host.call(999, &[], &mut vm).is_err());
    assert!(host.call(741, &[], &mut vm).is_err());
    assert!(host.call(1050, &[], &mut vm).is_err());
    host.system_settings = Some(settings());
    for arguments in [&[][..], &[0xff; 3][..], &[0xff; 16][..]] {
        assert_eq!(
            host.call(999, arguments, &mut vm).unwrap(),
            NativeReturn::Value(0x80000007)
        );
        assert_eq!(
            host.call(741, arguments, &mut vm).unwrap(),
            NativeReturn::Value(0x80000003)
        );
        assert_eq!(
            host.call(1050, arguments, &mut vm).unwrap(),
            NativeReturn::Value(0x8000_0005)
        );
    }
    host.system_settings.as_mut().unwrap().fullscreen = 0;
    assert_eq!(
        host.call(741, &[], &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    host.call(359, &arguments(&[0, 8, 16, 24]), &mut vm)
        .unwrap();
    let mut expected = vec![0x11; 0x15fc];
    expected[8..80].fill(0x22);
    expected[16..48].fill(0x33);
    expected[24..36].fill(0x44);
    assert_eq!(vm.read(0, expected.len()).unwrap(), expected);
    assert_eq!(
        vm.read(0x15fc, 6000 - 0x15fc).unwrap(),
        vec![0xaa; 6000 - 0x15fc]
    );
    assert_eq!(vm.read(0x80000000, 16).unwrap(), [0xbb; 16]);

    vm.write(0, &[0x55; 0x15fc]).unwrap();
    assert!(
        host.call(360, &arguments(&[0, 0x80000008, u32::MAX]), &mut vm)
            .is_err()
    );
    let state = host.system_settings.as_ref().unwrap();
    assert_eq!(state.sound, [0x55; 0x15fc]);
    assert_eq!(state.message, [0x22; 0x48]);
    assert_eq!(state.window, [0x44; 0x0c]);
    let snapshot = state.clone();
    assert!(host.call(360, &arguments(&[0; 4]), &mut vm).is_err());
    assert_eq!(host.system_settings.as_ref().unwrap(), &snapshot);
    assert!(
        host.call(
            359,
            &arguments(&[u32::MAX, u32::MAX, u32::MAX, 5990]),
            &mut vm
        )
        .is_err()
    );
}

struct Source;
impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, words: &[u32]) {
            code.push(op);
            for word in words {
                code.push(3);
                code.extend(word.to_le_bytes());
            }
        }
        let mut code = vec![];
        ins(&mut code, 0x25, &[0xdeadbeef]);
        for value in [0, 5800, 5900] {
            ins(&mut code, 0x25, &[value]);
        }
        ins(&mut code, 0x2b, &[360, 12]);
        for value in [0, 0x80000000, 0x80000080, 0x800000c0] {
            ins(&mut code, 0x25, &[value]);
        }
        ins(&mut code, 0x2b, &[359, 16]);
        ins(&mut code, 0x26, &[6300]);
        ins(&mut code, 0x2b, &[999, 0]);
        ins(&mut code, 0x26, &[6304]);
        ins(&mut code, 0x2b, &[741, 0]);
        ins(&mut code, 0x26, &[6308]);
        ins(&mut code, 0x2b, &[1050, 0]);
        ins(&mut code, 0x26, &[6312]);
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0; 6400];
        data[..0x15fc].fill(0x31);
        data[5800..5800 + 0x48].fill(0x52);
        data[5900..5900 + 0x20].fill(0x73);
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

#[test]
fn ordinary_settings_writeback_and_external_reads_share_runtime_state() {
    for budget in [1, 4096] {
        let mut factory = KcsFactory::new(Source);
        factory.external = katze_system2::memory::SharedMemory::new(vec![0; 512]).unwrap();
        let mut rt = Runtime::new(
            0,
            kind::BASE,
            Box::new(BaseObject::new()),
            State::new(0),
            factory,
        );
        rt.set_budget(budget, 1 << 20);
        rt.host_mut().system_settings = Some(settings());
        let task = rt.create(0, kind::ORDINARY, b"settings", b"").unwrap();
        rt.update(0, 0).unwrap();
        let state = rt.host().system_settings.as_ref().unwrap();
        assert_eq!(state.sound, [0x31; 0x15fc]);
        assert_eq!(state.message, [0x52; 0x48]);
        assert_eq!(state.effects, [0x73; 0x20]);
        assert_eq!(state.window, [0x44; 0x0c]);
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(vm.read(6300, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
        assert_eq!(vm.read(6304, 4).unwrap(), 0x80000007u32.to_le_bytes());
        assert_eq!(vm.read(6308, 4).unwrap(), 0x80000003u32.to_le_bytes());
        assert_eq!(vm.read(6312, 4).unwrap(), 0x80000005u32.to_le_bytes());
        let mut expected = [0; 512];
        expected[..0x48].fill(0x52);
        expected[0x80..0xa0].fill(0x73);
        expected[0xc0..0xcc].fill(0x44);
        assert_eq!(vm.read(0x80000000, 512).unwrap(), expected);
    }
}

struct ConfigurationDevice(katze_system2::input::DeviceKeys);

impl katze_system2::input::InputDevice for ConfigurationDevice {
    fn key_configuration(&self) -> Option<&katze_system2::input::DeviceKeys> {
        Some(&self.0)
    }
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<katze_system2::input::PointerSample> {
        Ok(Default::default())
    }
    fn key(
        &mut self,
        key: u32,
        group: u32,
        repeat: bool,
    ) -> Result<katze_system2::input::KeySample> {
        self.0.sample(key, group, repeat)
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(1)
    }
    fn set_pointer(&mut self, _: [i32; 2]) -> Result<()> {
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn runtime_configuration_save_reads_current_device_bindings_and_preserves_the_vm_stack() {
    use katze_system2::{
        input::{DeviceBinding, DeviceKeys},
        settings::PlatformSettings,
    };
    struct SaveSource;
    impl ProgramLoader for SaveSource {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            for (opcode, values) in [
                (0x25, &[0xdeadbeef][..]),
                (0x2b, &[361, 0]),
                (0x26, &[0]),
                (0x3b, &[]),
            ] {
                code.push(opcode);
                for value in values {
                    code.push(3);
                    code.extend(u32::to_le_bytes(*value));
                }
            }
            Ok(Some(Program::from_code(&code, vec![0; 16], 16)?))
        }
    }
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::BASE,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(SaveSource),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().system_settings = Some(SystemSettings::default());
        runtime.host_mut().platform_settings = Some(PlatformSettings::default());
        runtime.host_mut().configuration_keys = Some(DeviceKeys::default());
        let writes = ConfigurationWrites::default();
        runtime.set_configuration_services(ConfigurationSink(writes.clone()));
        for code in [65, 66] {
            let mut bindings = [DeviceBinding::default(); 80];
            bindings[0].enabled = 1;
            bindings[0].codes[0][0] = code;
            runtime
                .set_input_device(ConfigurationDevice(DeviceKeys::new(bindings)))
                .unwrap();
            let task = runtime.create(0, kind::ORDINARY, b"save", b"").unwrap();
            runtime.update(0, 0).unwrap();
            assert_eq!(
                runtime
                    .object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .read(0, 4)
                    .unwrap(),
                0xdeadbeefu32.to_le_bytes()
            );
            let writes = writes.borrow();
            let (writer, _, bytes) = writes.last().unwrap();
            assert_eq!(*writer, task);
            assert_eq!(
                XmlDocument::parse(bytes)
                    .unwrap()
                    .integer(b"KEY/kb1_00", 0)
                    .unwrap(),
                u32::from(code)
            );
        }
        assert_eq!(writes.borrow().len(), 2);
    }
}
