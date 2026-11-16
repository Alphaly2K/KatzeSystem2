use katze_system2::{
    Result,
    expression::State,
    kcs::{
        ClipboardRequest, ClipboardText, CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask,
        Program,
    },
};

fn instruction(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for value in arguments {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn clipboard_requests_preserve_format_encoding_and_stack_across_resumption() -> Result<()> {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x1357_9bdf]);
    for (native, address) in [(10, 128), (1039, 0x8000_0000)] {
        instruction(&mut code, 0x25, &[address]);
        instruction(&mut code, 0x2b, &[native, 4]);
    }
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[32..36].copy_from_slice(&123u32.to_le_bytes());
    let source = b"\x81\\n%u[G32] %% %c[G36]ignored\0";
    data[128..128 + source.len()].copy_from_slice(source);
    let external = "Aあ🐈B\\n%u[G32]\0".as_bytes().to_vec();
    let program = Program::from_code(&code, data.clone(), 64)?;
    for budget in [1, 100] {
        let mut task = OrdinaryTask::new(program.clone(), external.clone())?;
        task.task_id = 19;
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
        assert_eq!(
            host.clipboard_requests,
            [
                ClipboardRequest {
                    task_id: 19,
                    text: ClipboardText::Bytes(b"\x81\r\n123 % ".to_vec()),
                },
                ClipboardRequest {
                    task_id: 19,
                    text: ClipboardText::Utf16(vec![65, 0x3042, 0xd83d, 66, 13, 10, 49, 50, 51]),
                },
            ]
        );
        assert_eq!(task.interpreter.read(0, 256)?, data);
        assert_eq!(
            task.interpreter.read(0x8000_0000, external.len())?,
            external
        );
        assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
    }
    Ok(())
}

#[test]
fn clipboard_conversion_handles_sentinel_and_stops_before_invalid_continuation() -> Result<()> {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0; 4096], 0)?, vec![])?;
    let address = 0u32.to_le_bytes();
    for (source, expected) in [
        (b"".as_slice(), vec![]),
        (b"A\xff\xffB\\n", vec![65, 0xffff, 66, 13, 10]),
        (b"A\x80B", vec![65]),
    ] {
        vm.write(0, &[source, &[0]].concat())?;
        assert_eq!(host.call(1039, &address, &mut vm)?, NativeReturn::Continue);
        assert_eq!(
            host.clipboard_requests.last().unwrap().text,
            ClipboardText::Utf16(expected)
        );
    }
    vm.write(0, &[vec![b'x'; 2047], vec![0]].concat())?;
    host.call(1039, &address, &mut vm)?;
    let before = host.clipboard_requests.clone();
    for source in [vec![b'x'; 2048], vec![0xe1, 0x80]] {
        vm.write(0, &[source, vec![0]].concat())?;
        assert!(host.call(1039, &address, &mut vm).is_err());
        assert_eq!(host.clipboard_requests, before);
    }
    assert!(host.call(10, &[], &mut vm).is_err());
    assert_eq!(host.clipboard_requests, before);
    Ok(())
}
