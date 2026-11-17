use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

#[test]
fn name_pairs_use_first_match_and_preserve_unwritten_bytes() -> Result<()> {
    use NativeReturn::{Continue, Value};

    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(
        Program::from_code(&[0x3b], vec![0xcc; 256], 64)?,
        vec![0xee; 32],
    )?;
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, index, args: &[u32]| {
        let args: Vec<_> = args.iter().flat_map(|word| word.to_le_bytes()).collect();
        host.call(index, &args, vm)
    };
    let Value(table) = call(&mut host, &mut vm, 289, &[1, 2, 2])? else {
        panic!("expected raw buffer address");
    };
    assert_eq!(host.raw_buffer(table, 160)?, &[0; 160]);
    assert!(host.raw_buffer(table, 161).is_err());

    vm.write(0, b"\x82\xa0Name%u[G40]\0")?;
    vm.write(16, b"\x82\xa0nAME7\0Other\0")?;
    vm.write(40, &7u32.to_le_bytes())?;
    vm.write(48, b"\0missing\0")?;
    vm.write(64, b"12345678901234567890123456789012\0")?;

    assert_eq!(
        call(&mut host, &mut vm, 291, &[table, 48, 17, 18])?,
        Continue
    );
    assert_eq!(
        call(&mut host, &mut vm, 292, &[table, 48, 100, 104])?,
        Value(0)
    );
    assert_eq!(vm.read(100, 8)?, [0xcc; 8]);
    assert_eq!(host.raw_buffer(table + 32, 8)?, [17, 0, 0, 0, 18, 0, 0, 0]);

    for (name, first, second) in [(0, 5, u32::MAX), (16, 9, 10), (24, 5, u32::MAX)] {
        assert_eq!(
            call(&mut host, &mut vm, 291, &[table, name, first, second])?,
            Continue,
        );
    }
    assert_eq!(host.raw_buffer(table, 9)?, b"\x82\xa0Name7\0\0");
    assert_eq!(
        call(&mut host, &mut vm, 292, &[table, 16, 100, 0x8000_0000])?,
        Value(1)
    );
    assert_eq!(vm.read(100, 4)?, 5u32.to_le_bytes());
    assert_eq!(vm.read(0x8000_0000, 4)?, u32::MAX.to_le_bytes());
    assert_eq!(
        call(&mut host, &mut vm, 292, &[table + 40, 16, 104, 104])?,
        Value(1)
    );
    assert_eq!(vm.read(104, 4)?, 10u32.to_le_bytes());
    assert_eq!(
        call(&mut host, &mut vm, 292, &[table, 49, 100, 104])?,
        Value(0)
    );
    assert_eq!(vm.read(100, 8)?, [5, 0, 0, 0, 10, 0, 0, 0]);

    assert_eq!(
        call(&mut host, &mut vm, 293, &[table, 5, u32::MAX, 112])?,
        Value(1)
    );
    assert_eq!(vm.read(112, 9)?, b"\x82\xa0Name7\0\xcc");
    assert_eq!(
        call(&mut host, &mut vm, 293, &[table, 5, 0, 112])?,
        Value(0)
    );
    assert_eq!(vm.read(112, 9)?, b"\x82\xa0Name7\0\xcc");

    assert!(call(&mut host, &mut vm, 291, &[table, 64, 0, 0]).is_err());
    assert_eq!(call(&mut host, &mut vm, 290, &[table])?, Continue);
    assert!(call(&mut host, &mut vm, 293, &[table, 5, 0, 112]).is_err());
    for index in 291..=293 {
        assert_eq!(call(&mut host, &mut vm, index, &[0])?, Continue);
    }
    Ok(())
}
