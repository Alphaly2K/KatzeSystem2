use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program},
};

fn instruction(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for &value in arguments {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn debug_natives_keep_encoding_newline_and_stack_across_budget_resumption() -> Result<()> {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x1357_9bdf]);
    for native in [1, 1040] {
        instruction(&mut code, 0x25, &[128]);
        instruction(&mut code, 0x2b, &[native, 4]);
    }
    code.push(0x3b);
    let mut data = vec![0x55; 512];
    data[32..36].copy_from_slice(&123u32.to_le_bytes());
    data[36..40].fill(0);
    let text = "Aあ🐈B:%u[G32] a%%%%b %c[G36]ignored\0".as_bytes();
    data[128..128 + text.len()].copy_from_slice(text);
    let program = Program::from_code(&code, data.clone(), 64)?;
    for budget in [1, 100] {
        let mut task = OrdinaryTask::new(program.clone(), vec![0x66; 16])?;
        let mut state = State::new(0);
        let mut host = CoreHost::new(&mut state);
        let mut done = false;
        for _ in 0..32 {
            done = task.update(0, 1, &mut host, budget)?;
            if done {
                break;
            }
        }
        assert!(done);
        assert_eq!(host.debug_messages, ["Aあ🐈B:123 a%b ".as_bytes()]);
        let mut wide: Vec<_> = "Aあ".encode_utf16().collect();
        wide.push(0xd83d);
        wide.extend("B:123 a%b \n".encode_utf16());
        assert_eq!(host.unicode_debug_messages, [wide]);
        assert_eq!(task.interpreter.read(0, 512)?, data);
        assert_eq!(task.interpreter.read(0x8000_0000, 16)?, [0x66; 16]);
        assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
    }
    Ok(())
}

#[test]
fn debug_limits_and_unprovided_printf_arguments_preserve_existing_queues() -> Result<()> {
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0; 4096], 0)?, vec![])?;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let address = 0u32.to_le_bytes();
    for (length, native, success) in [
        (499, 1040, true),
        (500, 1040, false),
        (2046, 1, true),
        (2047, 1, false),
    ] {
        vm.write(0, &[vec![b'a'; length], vec![0]].concat())?;
        let before = vm.read(0, 4096)?.to_vec();
        let result = host.call(native, &address, &mut vm);
        if success {
            assert_eq!(result?, NativeReturn::Continue);
        } else {
            assert!(result.is_err());
        }
        assert_eq!(vm.read(0, 4096)?, before);
    }
    assert_eq!(host.debug_messages, [vec![b'a'; 499]]);
    assert_eq!(
        host.unicode_debug_messages,
        [[vec![u16::from(b'a'); 499], vec![10]].concat()]
    );
    for format in [b"%%%%d\0", b"%%%%C\0"] {
        vm.write(0, format)?;
        for native in [1, 1040] {
            assert!(host.call(native, &address, &mut vm).is_err());
        }
    }
    assert_eq!(host.debug_messages.len(), 1);
    assert_eq!(host.unicode_debug_messages.len(), 1);
    Ok(())
}
