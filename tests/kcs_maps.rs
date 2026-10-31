use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn call(host: &mut CoreHost<'_>, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    let bytes: Vec<_> = args.iter().flat_map(|value| value.to_le_bytes()).collect();
    host.call(index, &bytes, vm).unwrap()
}

fn create(host: &mut CoreHost<'_>, vm: &mut Interpreter) -> u32 {
    let NativeReturn::Value(handle) = call(host, vm, 515, &[]) else {
        panic!("dictionary handle expected")
    };
    assert_ne!(handle, 0);
    handle
}

#[test]
fn integer_maps_preserve_first_values_raw_keys_and_instance_lifetimes() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 128], 64).unwrap(), vec![]).unwrap();
    for (address, bytes) in [
        (0, b"cmd\0".as_slice()),
        (16, b"Cmd\0"),
        (32, b"%d[G8]\0"),
        (64, b"0\0"),
        (80, b"cmd\0trailer\0"),
    ] {
        vm.write(address, bytes).unwrap();
    }
    let first = create(&mut host, &mut vm);
    let second = create(&mut host, &mut vm);
    assert_ne!(first, second);
    for (key, value) in [(0, 7), (80, 9), (16, 11), (32, 0xdead_beef), (96, 0)] {
        assert_eq!(
            call(&mut host, &mut vm, 517, &[first, key, value]),
            NativeReturn::Continue
        );
    }
    for (key, value) in [(0, 7), (16, 11), (32, 0xdead_beef), (64, u32::MAX), (96, 0)] {
        assert_eq!(
            call(&mut host, &mut vm, 518, &[first, key]),
            NativeReturn::Value(value)
        );
    }
    assert_eq!(
        call(&mut host, &mut vm, 518, &[second, 0]),
        NativeReturn::Value(u32::MAX)
    );
    call(&mut host, &mut vm, 517, &[second, 0, 55]);
    assert_eq!(
        call(&mut host, &mut vm, 516, &[first]),
        NativeReturn::Continue
    );
    let args: Vec<_> = [first, 0].into_iter().flat_map(u32::to_le_bytes).collect();
    assert!(host.call(518, &args, &mut vm).is_err());
    let fresh = create(&mut host, &mut vm);
    assert_eq!(
        call(&mut host, &mut vm, 518, &[fresh, 0]),
        NativeReturn::Value(u32::MAX)
    );
    assert_eq!(
        call(&mut host, &mut vm, 518, &[second, 0]),
        NativeReturn::Value(55)
    );
}

#[test]
fn null_integer_map_operations_skip_key_address_resolution() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![], 64).unwrap(), vec![]).unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 517, &[0, u32::MAX, 7]),
        NativeReturn::Continue
    );
    assert_eq!(
        call(&mut host, &mut vm, 518, &[0, u32::MAX]),
        NativeReturn::Value(u32::MAX)
    );
    assert_eq!(call(&mut host, &mut vm, 516, &[0]), NativeReturn::Continue);
}
