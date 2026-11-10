use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
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
