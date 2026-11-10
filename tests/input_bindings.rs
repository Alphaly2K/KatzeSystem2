use katze_system2::{
    Result,
    input::{
        ButtonSource, DeviceBinding, DeviceKeys, KeyEventKind, KeySample, KeyServices,
        KeyTransition, Keys,
    },
};

#[test]
fn configured_bindings_preserve_shared_repeat_and_debug_state() -> Result<()> {
    let mut device = DeviceKeys::from_xml(
        br#"<?xml version="1.0" encoding="UTF-8"?><document><KEYCUSTOMIZE>
        <key00><name>confirm</name><keyboard1>257</keyboard1><rep_keyboard1>258</rep_keyboard1>
          <mouse1>5</mouse1><touch1>5</touch1>
          <rep_mouse1>259</rep_mouse1><rep_touch1>260</rep_touch1>
          <touch2>6</touch2><rep_touch2>261</rep_touch2>
          <joypad3>7</joypad3><rep_joypad3>262</rep_joypad3></key00>
        <key01><name></name><keyboard1>1</keyboard1></key01>
        <key02><keyboard1>1</keyboard1></key02>
        <key32><name>debug</name><keyboard4>9</keyboard4>
          <rep_keyboard4>263</rep_keyboard4></key32>
        <key79><name>last</name><keyboard2>8</keyboard2>
          <rep_keyboard2>264</rep_keyboard2></key79>
        </KEYCUSTOMIZE></document>"#,
    )?;
    assert_eq!(device.bindings()[0].codes[0], [1, 0, 0, 0]);
    assert_eq!(device.bindings()[0].repeat[0], [2, 0, 0, 0]);
    device.button(ButtonSource::Keyboard, 257, 1);
    assert_eq!(device.sample(0, 0, true)?, KeySample { mask: 1, repeat: 2 });
    assert_eq!(device.sample(1, 0, true)?, KeySample::default());
    assert_eq!(device.sample(2, 0, true)?, KeySample::default());

    device.button(ButtonSource::Mouse, 5, 7);
    device.button(ButtonSource::Touch, 5, 1);
    assert_eq!(
        device.sample(0, 0, true)?,
        KeySample {
            mask: 19,
            repeat: 3
        }
    );
    device.button(ButtonSource::Mouse, 6, 1);
    device.button(ButtonSource::Touch, 6, 1);
    assert_eq!(
        device.sample(0, 1, true)?,
        KeySample {
            mask: 18,
            repeat: 5
        }
    );
    assert_eq!(device.sample(0, 0, true)?.repeat, 5);
    device.button(ButtonSource::Joypad, 7, 1);
    assert_eq!(device.sample(0, 2, true)?, KeySample { mask: 4, repeat: 6 });
    assert_eq!(device.sample(0, 0, true)?.repeat, 6);

    device.button(ButtonSource::Keyboard, 9, 1);
    assert_eq!(device.sample(32, 3, true)?, KeySample::default());
    device.debug_mode = 1;
    device.button(ButtonSource::Keyboard, 9, 1);
    device.button(ButtonSource::Keyboard, 8, 1);
    assert_eq!(
        device.sample(79, 1, true)?,
        KeySample { mask: 1, repeat: 8 }
    );
    device.debug_mode = 0;
    device.button(ButtonSource::Keyboard, 9, 0);
    device.commit();
    assert_eq!(
        device.sample(32, 3, true)?,
        KeySample { mask: 1, repeat: 7 }
    );

    device.force(0, 2)?;
    device.clear_pressed();
    assert_eq!(device.sample(0, 3, true)?, KeySample { mask: 1, repeat: 6 });
    assert_eq!(
        device.sample(0, 2, false)?,
        KeySample { mask: 1, repeat: 0 }
    );
    device.commit();
    assert_eq!(device.sample(0, 0, true)?, KeySample { mask: 0, repeat: 6 });
    device.force(0, 1)?;
    let binding = device.bindings()[0];
    device.set_binding(0, binding)?;
    assert_eq!(device.sample(0, 0, true)?, KeySample::default());
    assert!(device.sample(80, 0, true).is_err());
    assert!(device.sample(0, 4, true).is_err());
    assert!(device.force(80, 1).is_err());
    assert!(device.set_binding(80, binding).is_err());
    Ok(())
}

struct Services {
    device: DeviceKeys,
    events: Vec<KeyTransition>,
}

impl KeyServices for Services {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample> {
        self.device.sample(key, group, true)
    }

    fn dispatch(&mut self, _keys: &mut Keys, transition: KeyTransition) -> Result<bool> {
        self.events.push(transition);
        Ok(false)
    }
}

#[test]
fn short_button_pulses_reach_scanning_and_extra_queries_consume_forcing() -> Result<()> {
    let mut bindings = [DeviceBinding::default(); 80];
    bindings[0].enabled = 1;
    bindings[0].codes[ButtonSource::Keyboard as usize][0] = 13;
    bindings[0].codes[ButtonSource::Mouse as usize][1] = 1;
    let mut services = Services {
        device: DeviceKeys::new(bindings),
        events: Vec::new(),
    };
    let mut keys = Keys::default();
    services.device.button(ButtonSource::Keyboard, 13, 1);
    services.device.button(ButtonSource::Keyboard, 13, 0);
    services.device.button(ButtonSource::Mouse, 1, 1);
    services.device.button(ButtonSource::Mouse, 1, 0);
    keys.scan(&mut services)?;
    let changes = |events: &[KeyTransition]| {
        events
            .iter()
            .map(|event| {
                assert_eq!(event.kind, KeyEventKind::Change);
                (event.key, event.group, event.pressed, event.mask)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(changes(&services.events), [(0, 0, 1, 1), (0, 1, 1, 2)]);
    services.events.clear();
    services.device.commit();
    keys.scan(&mut services)?;
    assert_eq!(changes(&services.events), [(0, 0, 0, 1), (0, 1, 0, 2)]);

    services.events.clear();
    services.device.force(0, 2)?;
    assert_eq!(services.device.sample(0, 3, false)?.mask, 1);
    keys.scan(&mut services)?;
    assert_eq!(changes(&services.events), [(0, 0, 1, 1)]);
    services.events.clear();
    services.device.commit();
    keys.scan(&mut services)?;
    assert_eq!(changes(&services.events), [(0, 0, 0, 1)]);
    services.events.clear();
    services.device.commit();
    keys.scan(&mut services)?;
    assert!(services.events.is_empty());
    Ok(())
}

#[test]
fn user_key_overrides_preserve_definitions_touch_and_pending_samples() -> Result<()> {
    use katze_system2::resource::{XmlDocument, XmlWriter};
    let mut bindings = [DeviceBinding::default(); 80];
    bindings[0] = DeviceBinding {
        enabled: 0x8000_0000,
        codes: [[7, 8, 9, 10]; 4],
        repeat: [[300, 301, 302, 303]; 4],
    };
    bindings[79] = DeviceBinding {
        enabled: 1,
        ..bindings[0]
    };
    let mut keys = DeviceKeys::new(bindings);
    keys.button(ButtonSource::Keyboard, 7, 1);
    keys.apply_user_xml(
        br#"<?xml version="1.0" encoding="UTF-8"?><document><KEY>
      <kb1_00>257</kb1_00><ms4_00>-1</ms4_00><jp2r_00>258</jp2r_00>
      <kb1_01>99</kb1_01><touch1_00>88</touch1_00><kb4_79>259</kb4_79>
    </KEY></document>"#,
    )?;
    assert_eq!(keys.bindings()[0].enabled, 0x8000_0000);
    assert_eq!(keys.bindings()[0].codes[0], [1, 8, 9, 10]);
    assert_eq!(keys.bindings()[0].codes[1][3], 255);
    assert_eq!(keys.bindings()[0].repeat[2], [44, 2, 46, 47]);
    assert_eq!(keys.bindings()[0].codes[3], bindings[0].codes[3]);
    assert_eq!(keys.bindings()[0].repeat[3], bindings[0].repeat[3]);
    assert_eq!(keys.bindings()[1], bindings[1]);
    assert_eq!(keys.bindings()[79].codes[0][3], 3);
    assert_eq!(
        keys.sample(0, 0, true)?,
        KeySample {
            mask: 1,
            repeat: 300
        }
    );
    keys.commit();
    assert_eq!(
        keys.sample(0, 0, true)?,
        KeySample {
            mask: 1,
            repeat: 300
        }
    );
    let mut writer = XmlWriter::new();
    keys.write_xml(&mut writer)?;
    let bytes = writer.to_bytes()?;
    let xml = XmlDocument::parse(&bytes)?;
    assert_eq!(xml.integer(b"KEY/jp2r_00", 0)?, 2);
    assert_eq!(xml.text(b"KEY/kb1_01")?, None);
    assert_eq!(xml.text(b"KEY/touch1_00")?, None);
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.find("<jp1r_00>").unwrap() < text.find("<jp1_00>").unwrap());
    let mut restored = DeviceKeys::new(bindings);
    restored.apply_user_xml(&bytes)?;
    assert_eq!(restored.bindings(), keys.bindings());
    Ok(())
}
