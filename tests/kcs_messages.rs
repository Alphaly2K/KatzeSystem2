use katze_system2::{
    Step,
    expression::State,
    kcs::{CoreHost, Event, Interpreter, Program},
};

fn ins(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for argument in arguments {
        code.push(3);
        code.extend(argument.to_le_bytes());
    }
}

fn waiting_program(second_wait: bool, address: u32) -> Program {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[address]);
    ins(&mut code, 0x2b, &[71, 4]);
    ins(&mut code, 0x26, &[0]);
    if second_wait {
        ins(&mut code, 0x1e, &[address + 4, 3]);
        ins(&mut code, 0x26, &[4]);
        ins(&mut code, 0x25, &[64]);
        ins(&mut code, 0x2b, &[71, 4]);
        ins(&mut code, 0x26, &[8]);
        ins(&mut code, 0x1e, &[68, 7]);
        ins(&mut code, 0x26, &[12]);
    }
    ins(&mut code, 0x2b, &[65, 0]);
    code.push(0x3b);
    Program::from_code(&code, vec![0; 128], 128).unwrap()
}

fn event(index: u32) -> Step<Event> {
    Step::Event(Event {
        native_index: index,
        status: 4,
    })
}

#[test]
fn delivery_resumes_once_and_reads_back_from_the_latest_wait_address() {
    let mut vm = Interpreter::new(waiting_program(true, 32), vec![]).unwrap();
    vm.write(68, &99u32.to_le_bytes()).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    assert_eq!(vm.resume(&mut host, 100).unwrap(), event(71));
    assert!(vm.is_waiting_for_message());
    let mut message = [10, 20, 30, 40];
    assert_eq!(
        vm.dispatch_message(&mut message, &mut host, 1).unwrap(),
        Some(Step::Yield)
    );
    assert_eq!(vm.read(0, 4).unwrap(), 1u32.to_le_bytes());
    assert_eq!(vm.read(32, 4).unwrap(), 10u32.to_le_bytes());
    assert!(!vm.is_waiting_for_message());
    assert!(vm.resume(&mut host, 100).is_err());
    assert_eq!(
        vm.dispatch_message(&mut message, &mut host, 100).unwrap(),
        Some(event(71))
    );
    assert!(vm.is_waiting_for_message());
    assert_eq!(message, [10, 99, 30, 40]);
    assert_eq!(vm.read(36, 4).unwrap(), 23u32.to_le_bytes());
    assert!(vm.stack_bytes().is_empty());

    message = [50, 60, 70, 80];
    assert_eq!(
        vm.dispatch_message(&mut message, &mut host, 100).unwrap(),
        Some(event(65))
    );
    assert_eq!(message, [50, 67, 70, 80]);
    assert_eq!(vm.read(8, 4).unwrap(), 1u32.to_le_bytes());
    assert!(!vm.is_waiting_for_message());
    assert_eq!(
        vm.dispatch_message(&mut message, &mut host, 100).unwrap(),
        None
    );
    assert_eq!(vm.resume(&mut host, 100).unwrap(), Step::Halted);
}

#[test]
fn ordinary_resume_returns_zero_and_preserves_the_external_message_buffer() {
    let initial: Vec<_> = (0u8..20).collect();
    let mut vm = Interpreter::new(waiting_program(false, 0x8000_0004), initial.clone()).unwrap();
    assert!(vm.wait_for_message(0x8000_0005).is_err());
    assert!(!vm.is_waiting_for_message());
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    assert_eq!(vm.resume(&mut host, 100).unwrap(), event(71));
    assert_eq!(vm.resume(&mut host, 0).unwrap(), Step::Yield);
    assert!(!vm.is_waiting_for_message());
    assert_eq!(vm.stack_bytes(), 0u32.to_le_bytes());
    assert_eq!(vm.resume(&mut host, 100).unwrap(), event(65));
    assert_eq!(vm.read(0, 4).unwrap(), 0u32.to_le_bytes());
    assert!(vm.stack_bytes().is_empty());
    assert_eq!(vm.read(0x8000_0000, 20).unwrap(), initial);
}
