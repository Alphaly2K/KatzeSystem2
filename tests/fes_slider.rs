use katze_system2::fes::{ObjectAttributes, ObjectReference, SliderState, SliderUpdates};

#[test]
fn slider_initialization_keeps_its_bank_priority_rule_and_independent_requests() {
    let mut state = SliderState::default();
    assert!(
        state
            .apply(
                &ObjectAttributes {
                    file: Some(b"control\0ignored".to_vec()),
                    plane: Some(ObjectReference {
                        name: "panel".into(),
                        bank: 1,
                    }),
                    position_resource: Some(b"position:0x10tail".to_vec()),
                    priority: Some(7),
                    sequential_priority: true,
                    display: Some(0),
                    enabled: Some(1),
                    position: [Some(99), Some(88), None],
                    size: Some([10, 20]),
                    color: Some(0x12345678),
                    ..Default::default()
                },
                2,
            )
            .unwrap()
    );
    assert_eq!(state.file, b"control");
    assert_eq!(state.priority, 27);
    assert_eq!(state.rectangle, [0; 4]);
    assert_eq!(state.position, 0);
    assert_eq!(state.alpha, 255);
    assert_eq!(state.position_resource.as_ref().unwrap().frame, 16);
    assert_eq!(
        state.updates,
        SliderUpdates {
            file: true,
            metadata: true,
            dirty: true,
            ..Default::default()
        }
    );

    state.updates = Default::default();
    assert!(
        !state
            .apply(
                &ObjectAttributes {
                    priority: Some(7),
                    ..Default::default()
                },
                2,
            )
            .unwrap()
    );
    assert_eq!(state.priority, 29);
    assert!(!state.set_enabled(1));
    assert_eq!(state.updates, SliderUpdates::default());
    assert!(
        !state
            .apply(
                &ObjectAttributes {
                    enabled: Some(-2),
                    ..Default::default()
                },
                2,
            )
            .unwrap()
    );
    assert_eq!(state.enabled, -2);
    assert!(state.updates.dirty);

    state.updates = Default::default();
    state.set_range([-1, -3]);
    state.set_position(i32::MAX, -7);
    assert_eq!(state.range, [-1, -3]);
    assert_eq!(state.position, i32::MAX);
    assert_eq!(state.notification, -7);
    assert!(state.updates.range && state.updates.position);
    assert!(!state.updates.dirty);

    assert!(
        state
            .apply(
                &ObjectAttributes {
                    file: Some(b"replacement".to_vec()),
                    plane: Some(ObjectReference {
                        name: "x".repeat(32),
                        bank: 0,
                    }),
                    priority: Some(99),
                    ..Default::default()
                },
                2,
            )
            .is_err()
    );
    assert_eq!(state.file, b"replacement");
    assert_eq!(state.plane.name, "panel");
    assert_eq!(state.priority, 29);
}

#[test]
fn slider_motion_carries_only_alpha_and_repeats_the_update_even_when_unchanged() {
    let mut state = SliderState {
        range: [-3, 7],
        position: 2,
        rectangle: [10, 20, 30, 40],
        ..Default::default()
    };
    state.set_alpha(0x1234);
    let mut packed = [0; 36];
    packed[28] = 0x34000000;
    assert_eq!(state.motion_properties(), packed);
    let mut properties = [u32::MAX; 36];
    properties[28] = 0x81fedcba;
    for _ in 0..2 {
        state.updates = Default::default();
        state.apply_motion_properties(&properties);
        assert_eq!(state.alpha, 0x81);
        assert_eq!(state.rectangle, [10, 20, 30, 40]);
        assert_eq!(state.range, [-3, 7]);
        assert_eq!(state.position, 2);
        assert_eq!(
            state.updates,
            SliderUpdates {
                alpha: true,
                ..Default::default()
            }
        );
    }
}
