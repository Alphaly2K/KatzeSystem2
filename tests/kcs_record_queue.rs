use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

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
fn record_queue_copies_fifo_across_vm_regions_and_releases_handles() -> Result<()> {
    use NativeReturn::{Continue, Value};
    const EXTERNAL: u32 = 0x8000_0000;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![0xa5; 32], 0)?, vec![0xa5; 32])?;
    let Value(queue) = call(&mut host, &mut vm, 1010, &[3])? else {
        panic!("record queue handle expected");
    };
    let Value(other) = call(&mut host, &mut vm, 1010, &[0])? else {
        panic!("record queue handle expected");
    };
    assert_ne!(queue, other);

    for _ in 0..2 {
        assert_eq!(
            call(&mut host, &mut vm, 1012, &[other, EXTERNAL])?,
            Continue
        );
    }
    assert_eq!(
        call(&mut host, &mut vm, 1013, &[other, EXTERNAL])?,
        Value(1)
    );
    assert_eq!(vm.read(EXTERNAL, 1)?.as_ref(), b"\xa5");

    for (n, address) in (0..24).zip([0, EXTERNAL].into_iter().cycle()) {
        vm.write(address, &[n, 0, 255 - n])?;
        assert_eq!(call(&mut host, &mut vm, 1012, &[queue, address])?, Continue);
    }
    call(&mut host, &mut vm, 1012, &[queue, u32::MAX])?;
    assert!(call(&mut host, &mut vm, 1012, &[queue, EXTERNAL + 31]).is_err());
    assert_eq!(
        call(&mut host, &mut vm, 1013, &[queue, u32::MAX])?,
        Value(0)
    );
    assert!(call(&mut host, &mut vm, 1013, &[queue, EXTERNAL + 31]).is_err());
    for n in 0..24 {
        assert_eq!(
            call(&mut host, &mut vm, 1013, &[queue, EXTERNAL + 4])?,
            Value(1)
        );
        assert_eq!(vm.read(EXTERNAL + 4, 4)?.as_ref(), &[n, 0, 255 - n, 0xa5]);
    }
    assert_eq!(
        call(&mut host, &mut vm, 1013, &[queue, EXTERNAL + 4])?,
        Value(0)
    );
    assert_eq!(vm.read(EXTERNAL + 4, 4)?.as_ref(), &[23, 0, 232, 0xa5]);

    call(&mut host, &mut vm, 1012, &[queue, 0])?;
    assert_eq!(call(&mut host, &mut vm, 1013, &[queue, 4])?, Value(1));
    assert_eq!(vm.read(4, 4)?.as_ref(), &[22, 0, 233, 0xa5]);
    call(&mut host, &mut vm, 1011, &[queue])?;
    assert!(call(&mut host, &mut vm, 1013, &[queue, 0]).is_err());
    assert_eq!(call(&mut host, &mut vm, 1013, &[other, 0])?, Value(1));
    assert_eq!(call(&mut host, &mut vm, 1013, &[other, 0])?, Value(0));
    call(&mut host, &mut vm, 1011, &[other])?;
    assert_eq!(call(&mut host, &mut vm, 1011, &[0])?, Continue);
    assert_eq!(call(&mut host, &mut vm, 1012, &[0, u32::MAX])?, Continue);
    assert_eq!(call(&mut host, &mut vm, 1013, &[0])?, Value(0));
    Ok(())
}
