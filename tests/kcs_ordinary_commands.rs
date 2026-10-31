use katze_system2::{
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
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
    assert!(!task.request_delete(&mut host, 1).unwrap());
    assert_eq!(task.control.deletion_requested, 0);
    let mut message = [7, 8, 9, 10];
    let mut output = 99;
    assert!(
        task.dispatch_message(&mut message, &mut output, &mut host, 100)
            .is_err()
    );
    for _ in 0..20 {
        if task.request_delete(&mut host, 1).unwrap() {
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
