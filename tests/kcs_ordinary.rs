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
    ins(&mut code, 0x25, &[(3, 0x1357_9bdf)]);
    for native in [141, 169, 191, 543, 573, 642, 906, 933, 1025, 1048] {
        for value in [u32::MAX, 0x8000_0000] {
            ins(&mut code, 0x25, &[(3, value)]);
        }
        ins(&mut code, 0x2b, &[(3, native), (3, 8)]);
    }
    for (native, target) in [
        (68, 0),
        (5, 4),
        (4, 8),
        (465, 12),
        (938, 16),
        (1030, 20),
        (565, 24),
        (566, 28),
    ] {
        ins(&mut code, 0x2b, &[(3, native), (3, 0)]);
        ins(&mut code, 0x26, &[(3, target)]);
    }
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 32], 64).unwrap();
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
        let debug_mode = system_time_ms ^ 0x1234_5678;
        host.debug_mode = debug_mode;
        host.sound_config_enabled = task_id;
        host.frame_capture_enabled = task_id ^ debug_mode;
        host.frame_capture_index = now;
        assert!(task.update(now, 1, &mut host, 100).unwrap());
        assert_eq!(
            values(&task),
            [task_id, elapsed, system_time_ms, debug_mode]
        );
        assert_eq!(task.interpreter.read(16, 4).unwrap(), task_id.to_le_bytes());
        assert_eq!(task.interpreter.read(20, 4).unwrap(), [0; 4]);
        assert_eq!(
            task.interpreter.read(24, 4).unwrap(),
            (task_id ^ debug_mode).to_le_bytes()
        );
        assert_eq!(task.interpreter.read(28, 4).unwrap(), now.to_le_bytes());
        assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
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

#[test]
fn menu_requests_keep_task_identity_and_are_emitted_once_across_budget_resumption() {
    use katze_system2::kcs::MenuItemRequest;
    let mut code = Vec::new();
    for value in [0, u32::MAX] {
        ins(&mut code, 0x25, &[(3, value)]);
        ins(&mut code, 0x2b, &[(3, 393), (3, 4)]);
    }
    ins(&mut code, 0x2b, &[(3, 68), (3, 0)]);
    ins(&mut code, 0x26, &[(3, 0)]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 16], 64).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    for id in [7, 9] {
        let mut task = OrdinaryTask::new(program.clone(), vec![]).unwrap();
        task.task_id = id;
        let mut completed = false;
        for _ in 0..20 {
            completed = task.update(100, 1, &mut host, 1).unwrap();
            if completed {
                break;
            }
        }
        assert!(completed);
        assert!(task.interpreter.stack_bytes().is_empty());
        assert_eq!(values(&task)[0], id);
    }
    let expected: Vec<_> = [7, 9]
        .into_iter()
        .flat_map(|task_id| {
            [0, u32::MAX].into_iter().map(move |value| MenuItemRequest {
                task_id,
                command_id: 0x77,
                value,
            })
        })
        .collect();
    assert_eq!(host.menu_item_requests, expected);
}

#[test]
fn language_requests_keep_task_identity_and_stack_across_budget_resumption() {
    use katze_system2::language::{LanguageChangeRequest, LanguageTarget::*};
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0x1234_5678)]);
    for (mode, value) in [(0, 1), (1, 2), (2, 1), (1, 2), (3, 9)] {
        ins(&mut code, 0x25, &[(3, mode)]);
        ins(&mut code, 0x25, &[(3, value)]);
        ins(&mut code, 0x2b, &[(3, 1053), (3, 8)]);
    }
    ins(&mut code, 0x26, &[(3, 0)]);
    ins(&mut code, 0x2b, &[(3, 68), (3, 0)]);
    ins(&mut code, 0x26, &[(3, 4)]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 16], 128).unwrap();
    for budget in [1, 4096] {
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        for id in [7, 9] {
            host.languages = Default::default();
            let mut task = OrdinaryTask::new(program.clone(), vec![]).unwrap();
            task.task_id = id;
            let mut complete = false;
            for _ in 0..100 {
                complete = task.update(100, 1, &mut host, budget).unwrap();
                if complete {
                    break;
                }
            }
            assert!(complete);
            assert!(task.interpreter.stack_bytes().is_empty());
            assert_eq!(values(&task)[..2], [0x1234_5678, id]);
            assert_eq!(host.system_settings_dirty, 1);
        }
        assert_eq!(
            host.language_change_requests,
            [7, 9]
                .into_iter()
                .flat_map(|task_id| [Interface, MainText, Subtitle]
                    .map(|target| LanguageChangeRequest { task_id, target }))
                .collect::<Vec<_>>()
        );
    }
}
