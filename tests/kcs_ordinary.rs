use katze_system2::{
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
};

fn ins(code: &mut Vec<u8>, opcode: u8, arguments: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in arguments {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn message_loop() -> Program {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 32)]);
    ins(&mut code, 0x2b, &[(3, 71), (3, 4)]);
    ins(&mut code, 0x26, &[(3, 0)]);
    ins(&mut code, 0x1d, &[(3, 4), (3, 10)]);
    ins(&mut code, 0x2d, &[]);
    ins(&mut code, 0x1e, &[(3, 4), (0x13, 32)]);
    ins(&mut code, 0x2d, &[]);
    ins(&mut code, 0x1e, &[(3, 8), (3, 1)]);
    ins(&mut code, 0x2d, &[]);
    ins(&mut code, 0x2b, &[(3, 5), (3, 0)]);
    ins(&mut code, 0x26, &[(3, 12)]);
    ins(&mut code, 0x29, &[(3, 0)]);
    Program::from_code(&code, vec![0; 128], 128).unwrap()
}

fn values(task: &OrdinaryTask) -> Vec<u32> {
    task.interpreter
        .read(0, 16)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

#[test]
fn task_drains_messages_then_resumes_without_one_even_when_clock_is_paused() {
    let mut task = OrdinaryTask::new(message_loop(), vec![]).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    task.running = 0;
    task.enqueue_message([9, 0, 0, 0]).unwrap();
    assert!(task.update(100, 99, &mut host, 100).unwrap());
    assert_eq!(task.clock.time, 100);
    assert_eq!(task.accumulated_increment, 0);
    assert_eq!(task.queued_messages().len(), 1);
    task.running = 1;
    assert!(task.update(100, 2, &mut host, 100).unwrap());
    assert!(task.queued_messages().is_empty());
    assert_eq!(values(&task), [0, 0, 0, 0]);
    assert!(task.interpreter.is_waiting_for_message());

    task.clock.paused = -1;
    for value in [1, 2] {
        task.enqueue_message([value, 0, 0, 0]).unwrap();
    }
    assert!(task.update(200, 3, &mut host, 100).unwrap());
    assert_eq!(values(&task), [0, 122, 3, 0]);
    assert_eq!(task.accumulated_increment, 5);
    assert_eq!((task.clock.origin, task.clock.time), (100, 100));
    assert_eq!(task.running, 1);
    assert!(task.queued_messages().is_empty());
}

#[test]
fn budget_resumption_keeps_the_update_time_and_current_message() {
    let mut task = OrdinaryTask::new(message_loop(), vec![]).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    assert!(!task.update(100, 3, &mut host, 1).unwrap());
    assert!(task.is_pending());
    assert!(task.enqueue_message([9, 0, 0, 0]).is_err());
    assert!(task.update(999, 99, &mut host, 1).unwrap());
    assert_eq!(task.clock.time, 100);
    assert_eq!(task.accumulated_increment, 3);
    for value in [1, 2] {
        task.enqueue_message([value, 0, 0, 0]).unwrap();
    }
    assert!(!task.update(200, 5, &mut host, 1).unwrap());
    assert_eq!(task.queued_messages().len(), 2);
    for _ in 0..100 {
        if task.update(999, 99, &mut host, 1).unwrap() {
            break;
        }
    }
    assert!(!task.is_pending());
    assert_eq!(values(&task), [0, 122, 3, 200]);
    assert_eq!(task.clock.time, 200);
    assert_eq!(task.accumulated_increment, 8);
    assert!(task.queued_messages().is_empty());
}

#[test]
fn stopped_task_keeps_updating_its_clock_and_retains_queued_messages() {
    let mut code = Vec::new();
    ins(&mut code, 0x2b, &[(3, 65), (3, 0)]);
    ins(&mut code, 0x1e, &[(3, 0), (3, 1)]);
    ins(&mut code, 0x2d, &[]);
    code.push(0x3b);
    let mut task =
        OrdinaryTask::new(Program::from_code(&code, vec![0; 16], 64).unwrap(), vec![]).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    task.accumulated_increment = u32::MAX;
    assert!(task.update(10, 1, &mut host, 100).unwrap());
    assert_eq!(task.running, 1);
    assert_eq!(task.accumulated_increment, 0);
    assert!(task.update(20, 2, &mut host, 100).unwrap());
    assert_eq!(task.running, 0);
    task.enqueue_message([1, 2, 3, 4]).unwrap();
    assert!(task.update(30, 3, &mut host, 100).unwrap());
    assert_eq!(task.clock.time, 30);
    assert_eq!(task.accumulated_increment, 2);
    assert_eq!(task.queued_messages(), [[1, 2, 3, 4]]);
    assert_eq!(values(&task), [1, 0, 0, 0]);
}

#[test]
fn implicit_code_end_stops_on_the_following_update_across_budget_resumption() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 42)]);
    for budget in [1, 100] {
        let mut task =
            OrdinaryTask::new(Program::from_code(&code, vec![0; 16], 64).unwrap(), vec![]).unwrap();
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        for _ in 0..10 {
            if task.update(10, 1, &mut host, budget).unwrap() {
                break;
            }
        }
        assert!(!task.is_pending());
        assert_eq!(task.running, 1);
        assert_eq!(task.accumulated_increment, 1);
        assert_eq!(task.interpreter.stack_bytes(), 42u32.to_le_bytes());
        assert!(task.update(20, 2, &mut host, budget).unwrap());
        assert_eq!(task.running, 0);
        assert_eq!(task.accumulated_increment, 3);
        assert!(task.update(30, 3, &mut host, budget).unwrap());
        assert_eq!(task.accumulated_increment, 3);
    }
}

#[test]
fn shared_host_receives_each_task_id_and_keeps_system_milliseconds_independent() {
    let mut code = Vec::new();
    for (native, target) in [(68, 0), (5, 4), (4, 8)] {
        ins(&mut code, 0x2b, &[(3, native), (3, 0)]);
        ins(&mut code, 0x26, &[(3, target)]);
    }
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 16], 64).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    for (task_id, origin, now, system_time_ms, elapsed) in [
        (0xdead_beef, 10, 100, u32::MAX, 90),
        (0x8000_0000, u32::MAX, 5, 0, 6),
    ] {
        let mut task = OrdinaryTask::new(program.clone(), vec![]).unwrap();
        task.task_id = task_id;
        task.clock.origin = origin;
        host.system_time_ms = system_time_ms;
        assert!(task.update(now, 1, &mut host, 100).unwrap());
        assert_eq!(values(&task), [task_id, elapsed, system_time_ms, 0]);
        assert_eq!(task.running, 0);
    }
}

#[test]
fn split_update_preserves_clock_controls_and_messages_changed_by_children() {
    for budget in [1, 100] {
        let mut task = OrdinaryTask::new(message_loop(), vec![]).unwrap();
        task.clock.origin = 10;
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        assert!(task.update(100, 1, &mut host, 100).unwrap());
        task.clock.update(200);
        task.clock.paused = -1;
        task.clock.one_tick_pending = true;
        task.enqueue_message([7, 0, 0, 0]).unwrap();
        let clock = task.clock.clone();
        let mut completed = false;
        for _ in 0..100 {
            completed = task.update_body(3, &mut host, budget).unwrap();
            if completed {
                break;
            }
        }
        assert!(completed);
        assert_eq!(task.clock, clock);
        assert_eq!(task.accumulated_increment, 4);
        assert_eq!(values(&task)[3], 190);
        assert!(task.interpreter.is_waiting_for_message());
        assert!(task.queued_messages().is_empty());
        assert!(task.update(300, 5, &mut host, 100).unwrap());
        assert_eq!(values(&task)[3], 90);
        assert_eq!(task.accumulated_increment, 9);
        assert!(!task.clock.one_tick_pending);
    }
}
