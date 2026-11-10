use katze_system2::{
    Result,
    input::{ButtonSource, DeviceBinding, DeviceKeys, WheelButtons},
};

#[test]
fn wheel_pulses_release_replay_and_cancel_without_clearing_other_keys() -> Result<()> {
    for up in [false, true] {
        for cancel in [false, true] {
            let mut bindings = [DeviceBinding::default(); 80];
            bindings[0].enabled = 1;
            bindings[0].codes[ButtonSource::Mouse as usize][0] = if up { 6 } else { 7 };
            bindings[1].enabled = 1;
            bindings[1].codes[ButtonSource::Keyboard as usize][0] = 13;
            let mut keys = DeviceKeys::new(bindings);
            let mut wheel = WheelButtons::default();
            keys.button(ButtonSource::Keyboard, 13, 1);
            for _ in 0..4 {
                wheel.scroll(up, |code, pressed| {
                    keys.button(ButtonSource::Mouse, code, pressed);
                });
            }
            let expected = if cancel {
                [2, 0, 2, 0, 0, 0, 0, 0]
            } else {
                [2, 0, 2, 0, 2, 0, 0, 0]
            };
            for (frame, mask) in expected.into_iter().enumerate() {
                if cancel && frame == 2 {
                    wheel.cancel_repeat();
                }
                assert_eq!(keys.sample(0, 0, true)?.mask, mask, "frame {frame}");
                assert_eq!(keys.sample(1, 0, true)?.mask, 1);
                wheel.finish(|code, pressed| {
                    keys.button(ButtonSource::Mouse, code, pressed);
                });
                keys.commit();
            }
        }
    }
    Ok(())
}

#[test]
fn reversed_wheel_releases_both_directions_and_replays_only_the_last() {
    let mut wheel = WheelButtons::default();
    let mut events = Vec::new();
    for up in [true, true, true, true, false, false, false] {
        wheel.scroll(up, |code, pressed| events.push((code, pressed)));
    }
    assert_eq!(
        events,
        [(6, 1), (6, 1), (6, 1), (6, 1), (7, 1), (7, 1), (7, 1)]
    );
    for expected in [vec![(6, 0), (7, 0)], vec![(7, 1)], vec![(7, 0)]] {
        events.clear();
        wheel.finish(|code, pressed| events.push((code, pressed)));
        assert_eq!(events, expected);
    }
    wheel.cancel_repeat();
    events.clear();
    wheel.finish(|code, pressed| events.push((code, pressed)));
    assert!(events.is_empty());
}
