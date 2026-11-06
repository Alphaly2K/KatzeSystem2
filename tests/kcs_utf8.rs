use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn vm() -> Result<Interpreter> {
    Interpreter::new(
        Program::from_code(&[], vec![0x55; 8192], 0)?,
        vec![0x55; 8192],
    )
}

fn call(
    host: &mut CoreHost<'_>,
    vm: &mut Interpreter,
    index: u32,
    args: &[u32],
) -> Result<NativeReturn> {
    let bytes: Vec<_> = args.iter().flat_map(|value| value.to_le_bytes()).collect();
    host.call(index, &bytes, vm)
}

#[test]
fn utf8_search_converts_cp932_and_advances_only_at_baseline_boundaries() -> Result<()> {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    for external in [0, 0x8000_0000] {
        for (source, needle, expected) in [
            ("aあb".as_bytes(), b"\x82\xa0".as_slice(), 1),
            ("あい".as_bytes(), b"\x82\xa2", 3),
            (b"abc", b"z", 3),
            (b"abc", b"", 0),
            (b"", b"a", 0),
            (b"\xc0a", b"a", 2),
            (b"\x80a", b"a", 2),
            ("a\u{f8f0}".as_bytes(), b"\xa0", 1),
            (" ・!".as_bytes(), b"\x82 ", 1),
        ] {
            let mut vm = vm()?;
            vm.write(external + 64, &[source, &[0]].concat())?;
            vm.write(1024, &[needle, &[0]].concat())?;
            assert_eq!(
                call(&mut host, &mut vm, 1051, &[external + 64, 1024])?,
                NativeReturn::Value(expected)
            );
            assert_eq!(
                vm.read(external + 64, source.len() + 1)?,
                [source, &[0]].concat()
            );
        }
    }
    let mut vm = vm()?;
    vm.write(64, b"abc\0")?;
    vm.write(1024, &[vec![b'a'; 4001], vec![0]].concat())?;
    assert_eq!(
        call(&mut host, &mut vm, 1051, &[64, 1024])?,
        NativeReturn::Value(0)
    );
    vm.write(1024, &[vec![b'a'; 260], vec![0]].concat())?;
    assert!(call(&mut host, &mut vm, 1051, &[64, 1024]).is_err());
    Ok(())
}

#[test]
fn utf8_truncation_keeps_exclusive_byte_limits_and_wrapping_character_steps() -> Result<()> {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    for external in [0, 0x8000_0000] {
        for (index, source, limit, expected) in [
            (1045, "AあB".as_bytes(), 0, 0),
            (1045, "AあB".as_bytes(), 4, 1),
            (1045, "AあB".as_bytes(), 5, 4),
            (1045, "AあB".as_bytes(), u32::MAX, 5),
            (1046, "AあB".as_bytes(), 0, 0),
            (1046, "AあB".as_bytes(), 1, 1),
            (1046, "AあB".as_bytes(), 2, 4),
            (1046, "AあB".as_bytes(), u32::MAX, 5),
            (1046, b"\xf8\x88\x80\x80\x80A", 1, 5),
            (1046, b"\xc0a", 1, 2),
            (1046, b"\xfc\x84\x80\x80\x80\x80A", 2, u32::MAX),
            (1046, b"\xfc\x84\x80\x80\x80\x80A", u32::MAX, 0),
        ] {
            let mut vm = vm()?;
            vm.write(external + 64, &[source, &[0]].concat())?;
            let mut expected_bytes = vm.read(external, 256)?.to_vec();
            let end = 64u32.wrapping_add(expected) as usize;
            expected_bytes[end..end + 2].fill(0);
            assert_eq!(
                call(&mut host, &mut vm, index, &[external + 64, limit])?,
                NativeReturn::Value(expected)
            );
            assert_eq!(vm.read(external, 256)?, expected_bytes);
        }
    }
    let mut vm = vm()?;
    vm.write(64, b"abc\xc0X\x80Z\0")?;
    assert!(call(&mut host, &mut vm, 1045, &[64, u32::MAX]).is_err());
    Ok(())
}

#[test]
fn cp932_conversion_keeps_private_bytes_and_capacity_failures_preserve_destination() -> Result<()> {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    for destination in [64, 1024, 0x8000_0040] {
        let mut vm = vm()?;
        vm.write(1024, b"\x80\xa0\xfd\xfe\xff\x82 \0")?;
        assert_eq!(
            call(&mut host, &mut vm, 1063, &[destination, 1024])?,
            NativeReturn::Continue
        );
        assert_eq!(
            vm.read_string(destination)?,
            "\u{80}\u{f8f0}\u{f8f1}\u{f8f2}\u{f8f3}・".as_bytes()
        );
    }
    for (source, expected) in [
        (vec![b'x'; 255], Some(vec![b'x'; 255])),
        (vec![b'x'; 256], None),
        (b"\x82\xa0".repeat(85), Some("あ".as_bytes().repeat(85))),
        (b"\x82\xa0".repeat(86), None),
        (b"%u[G32]".to_vec(), Some(b"%u[G32]".to_vec())),
        (Vec::new(), Some(Vec::new())),
    ] {
        let mut vm = vm()?;
        vm.write(1024, &[source, vec![0]].concat())?;
        call(&mut host, &mut vm, 1063, &[64, 1024])?;
        let mut expected_bytes = vec![0x55; 257];
        if let Some(bytes) = expected {
            expected_bytes[..bytes.len()].copy_from_slice(&bytes);
            expected_bytes[bytes.len()] = 0;
        }
        assert_eq!(vm.read(64, 257)?, expected_bytes);
    }
    Ok(())
}
