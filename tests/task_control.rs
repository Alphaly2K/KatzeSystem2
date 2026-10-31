use katze_system2::{
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
    task::{Command, Control, TaskClock, TaskTree},
};

fn command(code: u32, value: u32) -> Command {
    Command {
        target: 42,
        sender: 9,
        code,
        data: [123, value, 1],
    }
}

#[test]
fn deletion_gate_precedes_specific_handler_and_base_fallback_checks_it_once() {
    let mut control = Control::default();
    let mut clock = TaskClock::default();
    let mut output = 99;
    assert!(
        !control
            .dispatch(
                12,
                &mut clock,
                &command(13, 0),
                &mut output,
                |control, _, _, _| {
                    control.deletion_requested = i32::MIN;
                    Ok(true)
                }
            )
            .unwrap()
    );
    assert_eq!(output, 12);
    assert!(
        !control
            .dispatch(
                12,
                &mut clock,
                &command(24, 7),
                &mut output,
                |_, _, _, _| unreachable!()
            )
            .unwrap()
    );
    assert_eq!(control.property, u32::MAX);

    control.deletion_requested = 0;
    assert!(
        !control
            .dispatch(
                12,
                &mut clock,
                &command(29, 9),
                &mut output,
                |_, clock, _, out| {
                    clock.origin = 17;
                    *out = 33;
                    Ok(false)
                }
            )
            .unwrap()
    );
    assert_eq!(clock.paused, 0);
    assert_eq!(clock.origin, 17);
    assert_eq!(output, 33);
    assert!(
        control
            .dispatch(
                12,
                &mut clock,
                &command(173, 0),
                &mut output,
                |_, _, _, _| Ok(true)
            )
            .unwrap()
    );
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn routed_pause_updates_the_ordinary_task_clock_and_consumes_the_branch() {
    let mut code = Vec::new();
    ins(&mut code, 0x2b, &[5, 0]);
    ins(&mut code, 0x26, &[0]);
    ins(&mut code, 0x2b, &[65, 0]);
    ins(&mut code, 0x29, &[0]);
    let mut task =
        OrdinaryTask::new(Program::from_code(&code, vec![0; 16], 64).unwrap(), vec![]).unwrap();
    task.task_id = 42;
    task.clock.origin = 10;
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    assert!(task.update(100, 1, &mut host, 100).unwrap());
    assert_eq!(task.interpreter.read(0, 4).unwrap(), 90u32.to_le_bytes());

    let mut tree = TaskTree::new(9, 1);
    tree.insert(9, 42, 12).unwrap();
    tree.insert(42, 43, 12).unwrap();
    let mut output = 77;
    let mut calls = Vec::new();
    assert!(
        tree.dispatch(
            9,
            command(29, u32::MAX),
            &mut output,
            |tree, command, output| {
                calls.push(command.target);
                assert!(task.dispatch_control(
                    tree.node(command.target).unwrap().kind,
                    command,
                    output,
                    &mut host,
                    100,
                )?);
                Ok(false)
            }
        )
        .unwrap()
    );
    assert_eq!(calls, [42]);
    assert_eq!(output, 77);
    assert_eq!(task.clock.paused, -1);
    assert!(task.update(200, 1, &mut host, 100).unwrap());
    assert_eq!(task.interpreter.read(0, 4).unwrap(), (-10i32).to_le_bytes());
    assert_eq!(task.running, 1);

    for (code, value) in [(24, 12345), (23, 0), (3, 0), (24, 7)] {
        assert!(
            task.dispatch_control(12, &command(code, value), &mut output, &mut host, 100,)
                .unwrap()
        );
    }
    assert_eq!(output, 12345);
    assert_eq!(task.control.property, 12345);
    assert_eq!(task.control.deletion_requested, 1);
    assert!(tree.node(42).is_some());
}
