use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, TaskClock},
};

#[test]
fn pause_moves_origin_and_preserves_time_until_resume() {
    let mut clock = TaskClock {
        origin: 10,
        previous_time: 100,
        time: 100,
        paused: -1,
        one_tick_pending: true,
    };
    clock.update(110);
    assert_eq!(
        clock,
        TaskClock {
            origin: 20,
            previous_time: 110,
            time: 100,
            paused: -1,
            one_tick_pending: false,
        }
    );
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![], 64).unwrap(), vec![]).unwrap();
    host.set_task_clock(&clock);
    assert_eq!(host.call(5, &[], &mut vm).unwrap(), NativeReturn::Value(80));
    clock.paused = 0;
    clock.update(120);
    assert_eq!((clock.origin, clock.time, clock.elapsed()), (20, 120, 100));
    host.set_task_clock(&clock);
    assert_eq!(
        host.call(5, &[], &mut vm).unwrap(),
        NativeReturn::Value(100)
    );
}

#[test]
fn single_tick_is_consumed_once_and_wraps_at_zero_delta() {
    let mut clock = TaskClock {
        origin: 10,
        previous_time: 100,
        time: 100,
        one_tick_pending: true,
        ..TaskClock::default()
    };
    clock.update(150);
    assert_eq!((clock.origin, clock.elapsed()), (59, 91));
    assert!(!clock.one_tick_pending);
    clock.update(151);
    assert_eq!((clock.origin, clock.elapsed()), (59, 92));

    clock = TaskClock {
        origin: 0,
        previous_time: u32::MAX,
        time: u32::MAX,
        one_tick_pending: true,
        ..TaskClock::default()
    };
    clock.update(u32::MAX);
    assert_eq!((clock.origin, clock.elapsed()), (u32::MAX, 0));
    clock.update(0);
    assert_eq!(
        (clock.origin, clock.time, clock.elapsed()),
        (u32::MAX, 0, 1)
    );
}
