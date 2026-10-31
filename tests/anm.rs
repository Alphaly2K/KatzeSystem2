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
