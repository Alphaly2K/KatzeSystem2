use katze_system2::{Step, anm};

fn ins(opcode: u32, args: &[(u32, i32)]) -> anm::Instruction {
    let mut operands = [anm::Operand::default(); 8];
    for (i, &(kind, value)) in args.iter().enumerate() {
        operands[i] = anm::Operand {
            kind,
            value: value as u32,
        };
    }
    anm::Instruction { opcode, operands }
}

fn container(instructions: &[anm::Instruction]) -> Vec<u8> {
    let mut bytes = b"ANM\0".to_vec();
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((instructions.len() as u32).to_le_bytes());
    bytes.resize(32, 0);
    for instruction in instructions {
        bytes.extend(instruction.opcode.to_le_bytes());
        for operand in instruction.operands {
            bytes.extend(operand.kind.to_le_bytes());
            bytes.extend(operand.value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn random_ranges_exclude_upper_bound_and_keep_wrapping_order() {
    for (low, high, random, expected, draws) in [
        (2, 5, 0, 2, 1),
        (2, 5, 2, 4, 1),
        (2, 5, 3, 2, 1),
        (10, 2, 7, 17, 1),
        (5, 5, u32::MAX, 5, 0),
    ] {
        let program = anm::Program {
            instructions: vec![ins(0, &[(0, 10), (0, low), (0, high)])],
        };
        let mut vm = anm::Interpreter::new(program.clone());
        let mut calls = 0;
        assert_eq!(
            vm.run_with_random(10, &mut || {
                calls += 1;
                random
            })
            .unwrap(),
            Step::Event(anm::Event::Image {
                id: 10,
                duration: expected
            })
        );
        assert_eq!(calls, draws);
        if low != high {
            assert!(anm::Interpreter::new(program).run(10).is_err());
        }
    }
    let mut vm = anm::Interpreter::new(anm::Program {
        instructions: vec![
            ins(1, &[(0, 0), (0, -5), (0, 2)]),
            ins(15, &[(0, 2), (0, 2)]),
        ],
    });
    assert_eq!(
        vm.run_with_random(10, &mut || 6).unwrap(),
        Step::Event(anm::Event::Wait { duration: 2 })
    );
    assert_eq!(vm.variables[0], 1);
}

#[test]
fn baseline_loop_decrements_positive_values_and_jumps_otherwise() {
    for (initial, id, after) in [(3, 99, 2), (0, 88, 0), (-1, 88, -1)] {
        let mut vm = anm::Interpreter::new(anm::Program {
            instructions: vec![
                ins(2, &[(0, 0), (2, 2)]),
                ins(0, &[(0, 99), (0, 100), (0, 100)]),
                ins(0, &[(0, 88), (0, 100), (0, 100)]),
            ],
        });
        vm.variables[0] = initial;
        assert_eq!(
            vm.run(10).unwrap(),
            Step::Event(anm::Event::Image { id, duration: 100 })
        );
        assert_eq!(vm.variables[0], after);
    }
    let mut vm = anm::Interpreter::new(anm::Program {
        instructions: vec![
            ins(5, &[(0, 1000), (0, 1000), (2, 2)]),
            ins(0, &[(0, 99), (0, 100), (0, 100)]),
            ins(0, &[(0, 88), (0, 100), (0, 100)]),
        ],
    });
    assert_eq!(
        vm.run(10).unwrap(),
        Step::Event(anm::Event::Image {
            id: 88,
            duration: 100
        })
    );
}

#[test]
fn maximum_frame_and_display_properties_preserve_raw_values() {
    let mut vm = anm::Interpreter::new(anm::Program {
        instructions: vec![
            ins(11, &[(0, 3)]),
            ins(12, &[(1, 1)]),
            ins(13, &[(0, -2)]),
            ins(14, &[(0, -4), (1, 2)]),
            ins(0, &[(1, 3), (0, 1), (0, 1)]),
            ins(3, &[(2, 6)]),
        ],
    });
    vm.max_image_id = 77;
    vm.variables[1] = 300;
    vm.variables[2] = 7;
    assert_eq!(
        vm.run(20).unwrap(),
        Step::Event(anm::Event::Image {
            id: 77,
            duration: 1
        })
    );
    assert_eq!(
        vm.display,
        anm::DisplayState {
            image: 77,
            blend: 300,
            visible: -2,
            x: -4,
            y: 7
        }
    );
    assert_eq!(vm.run(20).unwrap(), Step::Halted);
}

#[test]
fn image_composition_uses_integer_offsets_and_unsigned_wrapping_blend() {
    let display = anm::DisplayState {
        image: 17,
        blend: 128,
        visible: -7,
        x: i32::MAX,
        y: -2,
    };
    assert_eq!(
        display.compose([1.75, -3.75], 128, 2),
        anm::DisplayState {
            image: 17,
            blend: 64,
            visible: -7,
            x: i32::MIN,
            y: -5,
        }
    );
    for (animation, image, expected) in [(-1, -1, 0), (-1, 255, 255), (65536, 65536, 0)] {
        let composed = anm::DisplayState {
            blend: animation,
            ..display
        }
        .compose([f32::NAN, f32::INFINITY], image, 0);
        assert_eq!(composed.blend, expected);
        assert_eq!(composed.visible, 0);
        assert_eq!((composed.x, composed.y), (-1, i32::MAX - 1));
    }
}

#[test]
fn timeline_catches_up_with_unscaled_origins_and_retains_signed_durations() {
    let mut timeline = anm::Timeline::new(anm::Program {
        instructions: (10..14)
            .map(|id| ins(0, &[(0, id), (0, 4), (0, 4)]))
            .collect(),
    });
    timeline.speed = 0.5;
    for (now, id, origin, changed, finished) in [
        (0, 10, 0, true, false),
        (20, 12, 8, true, false),
        (20, 13, 12, true, false),
        (20, 13, 12, false, true),
    ] {
        assert!(timeline.update(now, 100).unwrap());
        assert_eq!(timeline.interpreter.display.image, id);
        assert_eq!(timeline.origin, origin);
        assert_eq!(timeline.display_changed(), changed);
        assert_eq!(timeline.is_finished(), finished);
    }
    let mut timeline = anm::Timeline::new(anm::Program {
        instructions: vec![
            ins(0, &[(0, 10), (0, 0), (0, 0)]),
            ins(15, &[(0, -2), (0, -2)]),
            ins(12, &[(0, 30)]),
            ins(0, &[(0, 11), (0, 5), (0, 5)]),
        ],
    });
    assert!(timeline.update(0, 100).unwrap());
    assert_eq!((timeline.origin, timeline.duration), (u32::MAX - 1, 5));
    assert_eq!(
        (
            timeline.interpreter.display.image,
            timeline.interpreter.display.blend
        ),
        (11, 30)
    );
    assert!(!timeline.is_finished());
    assert!(timeline.update(3, 100).unwrap());
    assert!(timeline.is_finished());
    assert!(!timeline.display_changed());
    let mut timeline = anm::Timeline::new(anm::Program {
        instructions: vec![
            ins(0, &[(0, 8), (0, 1), (0, 1)]),
            ins(0, &[(0, 7), (0, 5), (0, 5)]),
        ],
    });
    timeline.interpreter.display.image = 7;
    assert!(timeline.update(1, 100).unwrap());
    assert!(!timeline.display_changed());
    for (now, speed) in [
        (1, f32::NAN),
        (1, f32::INFINITY),
        (1, -1.0),
        (0x8000_0000, 1.0),
    ] {
        let mut timeline = anm::Timeline::new(anm::Program {
            instructions: vec![ins(0, &[(0, 7), (0, 5), (0, 5)])],
        });
        timeline.speed = speed;
        assert!(timeline.update(now, 100).unwrap());
        assert_eq!(timeline.interpreter.pc, 0);
        assert!(!timeline.display_changed());
    }
}

#[test]
fn timeline_budget_resume_freezes_time_and_consumes_random_values_once() {
    let mut timeline = anm::Timeline::new(anm::Program {
        instructions: vec![
            ins(16, &[(0, 0), (0, 1)]),
            ins(0, &[(0, 7), (0, 2), (0, 5)]),
            ins(3, &[(0, 0)]),
        ],
    });
    let mut draws = 0;
    let mut random = || {
        draws += 1;
        0
    };
    assert!(!timeline.update_with_random(20, 1, &mut random).unwrap());
    assert!(timeline.is_pending());
    let mut complete = false;
    for _ in 0..100 {
        if timeline.update_with_random(999, 1, &mut random).unwrap() {
            complete = true;
            break;
        }
    }
    assert!(complete);
    assert!(!timeline.is_pending());
    assert_eq!((timeline.origin, timeline.duration), (20, 2));
    assert_eq!(timeline.interpreter.variables[0], 11);
    assert!(timeline.display_changed());
    assert!(timeline.update_with_random(20, 100, &mut random).unwrap());
    assert_eq!(draws, 11);
    assert!(!timeline.display_changed());
}

#[test]
fn object_clock_initializes_zero_origins_and_restart_preserves_variables_and_time() {
    let mut timeline = anm::Timeline::new(anm::Program {
        instructions: vec![
            ins(16, &[(0, 0), (0, 1)]),
            ins(0, &[(0, 10), (0, 4), (0, 4)]),
            ins(0, &[(0, 11), (0, 6), (0, 6)]),
        ],
    });
    timeline.speed = 0.5;
    timeline.interpreter.variables[2] = 77;
    timeline.interpreter.max_image_id = 42;
    assert!(timeline.update_frame(u32::MAX, 100).unwrap());
    assert_eq!(
        (timeline.origin, timeline.interpreter.display.image),
        (0, 10)
    );
    assert!(timeline.update_frame(100, 100).unwrap());
    assert_eq!(timeline.origin, 100);
    assert!(!timeline.display_changed());
    assert!(timeline.update_frame(108, 100).unwrap());
    assert_eq!(
        (timeline.origin, timeline.interpreter.display.image),
        (104, 11)
    );
    timeline.restart();
    assert_eq!(timeline.interpreter.display, anm::DisplayState::default());
    assert_eq!(
        (timeline.origin, timeline.speed, timeline.duration),
        (104, 0.5, 0)
    );
    assert_eq!(timeline.interpreter.variables[2], 77);
    assert_eq!(timeline.interpreter.max_image_id, 42);
    assert!(!timeline.update_frame(u32::MAX, 1).unwrap());
    assert!(timeline.is_pending());
    timeline.restart();
    assert!(!timeline.is_pending());
    assert!(timeline.update_frame(u32::MAX, 100).unwrap());
    assert_eq!(timeline.interpreter.variables[0], 3);
    assert_eq!(
        (timeline.origin, timeline.interpreter.display.image),
        (104, 10)
    );
}

#[test]
fn image_animation_reload_replaces_runtime_but_keeps_object_controls_and_failed_load_cache() {
    let mut bytes = container(&[ins(11, &[(0, 2)]), ins(0, &[(1, 2), (0, 5), (0, 5)])]);
    bytes.extend(b"tail");
    let mut image = anm::ImageAnimation::default();
    image.playing = 2;
    image.reset_time_on_update = true;
    image.time = 123;
    image.load(&bytes, 42).unwrap();
    image.timeline.as_mut().unwrap().speed = 0.5;
    image.timeline.as_mut().unwrap().interpreter.variables[5] = 77;
    assert!(image.update(100, 100, false).unwrap());
    assert_eq!(image.frame_change(), Some(42));
    assert_eq!(image.time, 100);
    image.reset_time_on_update = true;
    image.load(&bytes, 7).unwrap();
    let timeline = image.timeline.as_ref().unwrap();
    assert_eq!((timeline.origin, timeline.speed), (0, 1.0));
    assert_eq!(timeline.interpreter.variables, [0; 64]);
    assert_eq!(
        (image.time, image.playing, image.reset_time_on_update),
        (100, 2, true)
    );
    assert_eq!(image.display(), anm::DisplayState::default());
    assert!(image.update(112, 100, false).unwrap());
    assert_eq!(image.display().image, 7);
    let cached = image.display();
    assert!(image.load(&bytes[..bytes.len() - 5], 99).is_err());
    assert!(image.timeline.is_none());
    assert_eq!(image.display(), cached);
    assert!(image.update(999, 0, false).unwrap());
    assert_eq!(image.frame_change(), None);
    assert_eq!((image.time, image.playing), (112, 2));
}

#[test]
fn image_animation_budget_resume_preserves_sentinel_selection_and_delays_cached_display() {
    let bytes = container(&[
        ins(16, &[(0, 0), (0, 1)]),
        ins(12, &[(0, 70)]),
        ins(0, &[(0, 7), (0, 2), (0, 5)]),
    ]);
    let mut image = anm::ImageAnimation::default();
    image.load(&bytes, 77).unwrap();
    image.reset_time_on_update = true;
    assert!(image.update(100, 100, true).unwrap());
    assert!(image.reset_time_on_update);
    image.playing = 1;
    let mut draws = 0;
    let mut random = || {
        draws += 1;
        1
    };
    assert!(
        !image
            .update_with_random(u32::MAX, 1, true, &mut random)
            .unwrap()
    );
    assert_eq!(image.display(), anm::DisplayState::default());
    image.playing = 0;
    assert!(
        !image
            .update_with_random(999, 1, false, &mut random)
            .unwrap()
    );
    assert!(
        image
            .update_with_random(999, 1, false, &mut random)
            .unwrap()
    );
    assert_eq!(draws, 1);
    assert_eq!(image.frame_change(), Some(10000));
    assert_eq!(image.display().blend, 70);
    assert_eq!(image.time, u32::MAX);
    let timeline = image.timeline.as_ref().unwrap();
    assert_eq!(timeline.interpreter.display.image, 7);
    assert_eq!(timeline.interpreter.variables[0], 1);
    assert_eq!((timeline.origin, timeline.duration), (u32::MAX, 3));
    assert!(image.update(999, 0, true).unwrap());
    assert_eq!(image.frame_change(), None);
}
