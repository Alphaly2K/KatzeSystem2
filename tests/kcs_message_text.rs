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
fn extraction_keeps_raw_bytes_overlap_and_return_length_across_budget_resumption() -> Result<()> {
    for (native, source, expected, length) in [
        (
            392,
            b"[\x82\xa0/ruby]\\s\\n\\$256;Z%u[G32]%c[G36]ignored".as_slice(),
            b"\x82\xa0\x81\x40\x01\0Z123\0".as_slice(),
            5u32,
        ),
        (
            1049,
            "[あ/ruby]\\s\\n\\$256;Z%u[G32]%c[G36]ignored".as_bytes(),
            "あ\u{8140}\u{100}Z123\0".as_bytes(),
            12,
        ),
    ] {
        for destination in [0, 0x8000_0000, 128, 129] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0x1234_5678]);
            instruction(&mut code, 0x25, &[destination]);
            instruction(&mut code, 0x25, &[128]);
            instruction(&mut code, 0x2b, &[native, 8]);
            instruction(&mut code, 0x26, &[20]);
            code.push(0x3b);
            let mut initial = vec![0x55; 512];
            initial[32..36].copy_from_slice(&123u32.to_le_bytes());
            initial[36..40].fill(0);
            initial[128..128 + source.len()].copy_from_slice(source);
            initial[128 + source.len()] = 0;
            let program = Program::from_code(&code, initial.clone(), 64)?;
            for budget in [1, 100] {
                let mut task = OrdinaryTask::new(program.clone(), vec![0x66; 512])?;
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
                let mut normal = initial.clone();
                let mut external = vec![0x66; 512];
                let target = if destination >> 31 == 0 {
                    &mut normal
                } else {
                    &mut external
                };
                let offset = (destination & 0x7fff_ffff) as usize;
                target[offset..offset + expected.len()].copy_from_slice(expected);
                normal[20..24].copy_from_slice(&length.to_le_bytes());
                assert_eq!(task.interpreter.read(0, 512)?, normal);
                assert_eq!(task.interpreter.read(0x8000_0000, 512)?, external);
                assert_eq!(task.interpreter.stack_bytes(), 0x1234_5678u32.to_le_bytes());
            }
        }
    }
    Ok(())
}

#[test]
fn extraction_preserves_invalid_target_bypass_and_reports_unsupported_inputs() -> Result<()> {
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0x55; 512], 0)?, vec![])?;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    for native in [392, 1049] {
        let arguments: Vec<_> = [512u32, 128]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        vm.write(128, b"\\z\0")?;
        assert_eq!(
            host.call(native, &arguments, &mut vm)?,
            NativeReturn::Value(0)
        );
        let before = vm.read(0, 512)?.to_vec();
        let arguments: Vec<_> = [0u32, 128].into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(host.call(native, &arguments, &mut vm).is_err());
        assert_eq!(vm.read(0, 512)?, before);
        vm.write(128, b"abc\0")?;
        let arguments: Vec<_> = [510u32, 128]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let before = vm.read(0, 512)?.to_vec();
        assert!(host.call(native, &arguments, &mut vm).is_err());
        assert_eq!(vm.read(0, 512)?, before);
    }
    for (source, expected) in [
        (b"a\x80b\0".as_slice(), b"a\0".as_slice()),
        ("a🐈b\0".as_bytes(), "a\u{fffd}b\0".as_bytes()),
    ] {
        vm.write(128, source)?;
        let arguments: Vec<_> = [0u32, 128].into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(
            host.call(1049, &arguments, &mut vm)?,
            NativeReturn::Value((expected.len() - 1) as u32)
        );
        assert_eq!(vm.read(0, expected.len())?, expected);
    }
    Ok(())
}
