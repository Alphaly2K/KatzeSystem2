use katze_system2::{
    Step,
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
    task::Command,
};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

fn receiver(ending: &[u8]) -> OrdinaryTask {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[32]);
    ins(&mut code, 0x2b, &[71, 4]);
    ins(&mut code, 0x26, &[0]);
    ins(&mut code, 0x1e, &[36, 1]);
    ins(&mut code, 0x2d, &[]);
    ins(&mut code, 0x2b, &[68, 0]);
    ins(&mut code, 0x26, &[4]);
    code.extend_from_slice(ending);
    OrdinaryTask::new(Program::from_code(&code, vec![0; 64], 64).unwrap(), vec![]).unwrap()
}

#[test]
fn synchronous_delivery_keeps_task_state_and_defers_output_until_completion() {
    for (ending, expected) in [(&[][..], 1), (&[0x3b][..], 0)] {
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        let mut task = receiver(ending);
        assert!(task.update(100, 3, &mut host, 100).unwrap());
        task.task_id = 42;
        task.running = 0;
        let clock = task.clock.clone();
        let mut message = [7, 8, 9, 10];
        let mut output = 99;
        assert!(
            !task
                .dispatch_message(&mut message, &mut output, &mut host, 1)
                .unwrap()
        );
        assert_eq!((message, output), ([7, 8, 9, 10], 99));
        assert!(task.is_pending());
        assert!(task.update(200, 3, &mut host, 100).is_err());
        assert!(task.request_delete(&mut host, 100).is_err());
        assert!(task.enqueue_message([0; 4]).is_err());
        assert!(task.start_execution(42, 1).is_err());
        assert!(task.stop_execution().is_err());
        for _ in 0..20 {
            if task
                .dispatch_message(&mut message, &mut output, &mut host, 1)
                .unwrap()
            {
                break;
            }
        }
        assert!(!task.is_pending());
        assert_eq!((message, output), ([7, 9, 9, 10], expected));
        assert_eq!(task.interpreter.read(4, 4).unwrap(), 42u32.to_le_bytes());
        assert_eq!(task.clock, clock);
        assert_eq!((task.running, task.accumulated_increment), (0, 3));
    }
}

#[test]
fn deletion_notifies_before_marking_and_skips_subsequent_execution() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut task = receiver(&[0x3b]);
    task.task_id = 42;
    assert!(task.update(100, 3, &mut host, 100).unwrap());
    let clock = task.clock.clone();
    let command = Command {
        target: 42,
        sender: 7,
        code: 3,
        data: [0; 3],
    };
    assert!(
        !task
            .dispatch_control(12, &command, &mut 0, &mut host, 1)
            .unwrap()
    );
    assert_eq!(task.control.deletion_requested, 0);
    let mut message = [7, 8, 9, 10];
    let mut output = 99;
    assert!(
        task.dispatch_message(&mut message, &mut output, &mut host, 100)
            .is_err()
    );
    for _ in 0..20 {
        if task
            .dispatch_control(12, &command, &mut 0, &mut host, 1)
            .unwrap()
        {
            break;
        }
    }
    assert!(!task.is_pending());
    assert_eq!(task.control.deletion_requested, 1);
    assert_eq!(task.clock, clock);
    assert_eq!((task.running, task.accumulated_increment), (1, 3));
    let expected: Vec<_> = [u32::MAX, 0xffff_0007, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(task.interpreter.read(32, 16).unwrap(), expected);
    let pc = task.interpreter.pc;
    assert!(task.request_delete(&mut host, 0).unwrap());
    assert!(
        task.dispatch_message(&mut message, &mut output, &mut host, 0)
            .unwrap()
    );
    assert_eq!((message, output), ([7, 8, 9, 10], 99));
    assert!(task.update(200, 10, &mut host, 100).unwrap());
    assert_eq!(task.interpreter.pc, pc);
    assert_eq!((task.clock.time, task.accumulated_increment), (200, 3));
}

#[test]
fn paused_vm_preserves_waiting_but_still_receives_queued_messages() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut task = receiver(&[0x3b]);
    task.task_id = 42;
    assert!(task.update(10, 1, &mut host, 100).unwrap());
    let mut command = Command {
        target: 42,
        sender: 7,
        code: 162,
        data: [42, 0, 1],
    };
    assert!(
        task.dispatch_control(12, &command, &mut 0, &mut host, 0)
            .unwrap()
    );
    let pc = task.interpreter.pc;
    assert_eq!(task.interpreter.resume(&mut host, 0).unwrap(), Step::Paused);
    assert!(task.update(20, 2, &mut host, 0).unwrap());
    assert!(!task.is_pending());
    assert!(task.interpreter.is_waiting_for_message());
    assert_eq!(task.interpreter.pc, pc);
    assert!(task.interpreter.stack_bytes().is_empty());
    assert_eq!((task.running, task.accumulated_increment), (1, 3));
    task.enqueue_message([7, 8, 9, 10]).unwrap();
    for _ in 0..20 {
        if task.update(30, 3, &mut host, 1).unwrap() {
            break;
        }
    }
    assert!(!task.is_pending());
    assert!(!task.interpreter.is_waiting_for_message());
    assert_eq!(task.interpreter.read(0, 4).unwrap(), 1u32.to_le_bytes());
    assert_eq!(task.interpreter.read(36, 4).unwrap(), 9u32.to_le_bytes());
    assert_eq!((task.running, task.accumulated_increment), (1, 6));
    command.code = 163;
    assert!(
        task.dispatch_control(12, &command, &mut 0, &mut host, 0)
            .unwrap()
    );
    task.enqueue_message([3, 4, 5, 6]).unwrap();
    assert!(task.update(40, 4, &mut host, 100).unwrap());
    assert_eq!(task.queued_messages(), [[3, 4, 5, 6]]);
    command.code = 162;
    command.sender = 42;
    command.data[0] = 7;
    assert!(
        task.dispatch_control(12, &command, &mut 0, &mut host, 0)
            .unwrap()
    );
    assert_eq!(task.interpreter.execution_mode(), 1);
    assert!(task.update(50, 5, &mut host, 100).unwrap());
    assert_eq!(task.running, 0);
    assert!(task.queued_messages().is_empty());
    task.control.deletion_requested = -1;
    task.start_execution(42, 3).unwrap();
    assert_eq!((task.running, task.interpreter.execution_mode()), (0, 1));
}

#[test]
fn step_over_records_the_call_return_and_low_level_run_bypasses_pause() {
    let mut code = Vec::new();
    ins(&mut code, 0x2a, &[23, 0]);
    ins(&mut code, 0x2b, &[65, 0]);
    code.push(0x3b);
    ins(&mut code, 0x25, &[7]);
    ins(&mut code, 0x2c, &[4, 0]);
    let mut task =
        OrdinaryTask::new(Program::from_code(&code, vec![], 64).unwrap(), vec![]).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    task.start_execution(0, 3).unwrap();
    assert_eq!(task.interpreter.run(&mut host, 0).unwrap(), Step::Yield);
    assert_eq!(task.interpreter.execution_mode(), 3);
    assert_eq!(task.interpreter.run(&mut host, 1).unwrap(), Step::Yield);
    assert_eq!(task.interpreter.execution_mode(), 1);
    assert_eq!(task.interpreter.debug().step_over_return, Some(11));
    task.interpreter.set_execution_mode(0);
    assert_eq!(task.interpreter.debug().step_over_return, None);
    assert!(matches!(
        task.interpreter.run(&mut host, 100).unwrap(),
        Step::Event(_)
    ));
    assert_eq!(task.interpreter.stack_bytes(), 7u32.to_le_bytes());
    assert_eq!(
        task.interpreter.resume(&mut host, 100).unwrap(),
        Step::Paused
    );
    task.interpreter.set_execution_mode(u32::MAX);
    assert_eq!(
        task.interpreter.resume(&mut host, 100).unwrap(),
        Step::Halted
    );
}

#[test]
fn context_copy_reserves_the_terminator_and_preserves_state_on_capacity_errors() {
    let mut task = receiver(&[0x3b]);
    task.set_creation_context(Some(&[b'x'; 31])).unwrap();
    assert!(task.set_creation_context(Some(&[b'y'; 32])).is_err());
    assert_eq!(task.creation_context, [b'x'; 31]);
    let mut short = [0xa5; 31];
    assert!(task.copy_creation_context(Some(&mut short)).is_err());
    assert_eq!(short, [0xa5; 31]);
    let mut destination = [0xa5; 34];
    task.copy_creation_context(Some(&mut destination)).unwrap();
    assert_eq!(&destination[..31], &[b'x'; 31]);
    assert_eq!(&destination[31..], &[0, 0xa5, 0xa5]);
    task.set_creation_context(None).unwrap();
    task.copy_creation_context(None).unwrap();
    task.set_creation_context(Some(b"raw%d[G0]\0ignored"))
        .unwrap();
    assert_eq!(task.creation_context, b"raw%d[G0]");
    task.control.deletion_requested = -1;
    task.set_creation_context(Some(&[b'y'; 32])).unwrap();
    task.copy_creation_context(Some(&mut [])).unwrap();
    assert_eq!(task.creation_context, b"raw%d[G0]");
}
