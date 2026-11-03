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
    for (index, args, expected) in [
        (896, vec![0, u32::MAX, 7], NativeReturn::Continue),
        (897, vec![0, u32::MAX], NativeReturn::Value(0)),
        (898, vec![0], NativeReturn::Continue),
        (899, vec![0], NativeReturn::Value(0)),
        (902, vec![u32::MAX, 0, u32::MAX], NativeReturn::Value(0)),
        (895, vec![0], NativeReturn::Continue),
    ] {
        assert_eq!(call(&mut host, &mut vm, index, &args), expected);
    }
}

#[test]
fn named_values_share_dictionary_handles_and_keep_formatted_updates_and_sorted_keys() {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xaa; 256], 0).unwrap(),
        vec![0xbb; 64],
    )
    .unwrap();
    for (address, bytes) in [
        (0, 17u32.to_le_bytes().as_slice()),
        (16, b"%d[G0]\0"),
        (48, b"17\0"),
        (64, b"ABC\0"),
        (80, b"abc\0"),
        (96, b"missing\0"),
    ] {
        vm.write(address, bytes).unwrap();
    }
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 894, &[u32::MAX]) else {
        panic!("dictionary handle expected")
    };
    call(&mut host, &mut vm, 896, &[handle, 16, 1]);
    call(&mut host, &mut vm, 896, &[handle, 48, 0x80000001]);
    for (address, value) in [(16, 7), (64, 9), (80, 11)] {
        call(&mut host, &mut vm, 517, &[handle, address, value]);
    }
    assert_eq!(
        call(&mut host, &mut vm, 518, &[handle, 16]),
        NativeReturn::Value(7)
    );
    assert_eq!(
        call(&mut host, &mut vm, 897, &[handle, 16]),
        NativeReturn::Value(0x80000001)
    );
    assert_eq!(
        call(&mut host, &mut vm, 897, &[handle, 96]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 899, &[handle]),
        NativeReturn::Value(40)
    );
    for (index, key) in [b"%d[G0]".as_slice(), b"17", b"ABC", b"abc"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            call(&mut host, &mut vm, 902, &[handle, index as u32, 0x80000000]),
            NativeReturn::Value(1)
        );
        assert_eq!(vm.read_string(0x80000000).unwrap(), key);
    }
    let before = vm.read(0x80000000, 64).unwrap().to_vec();
    assert_eq!(
        call(&mut host, &mut vm, 902, &[handle, u32::MAX, 0x80000000]),
        NativeReturn::Value(0)
    );
    assert_eq!(vm.read(0x80000000, 64).unwrap(), before);
    call(&mut host, &mut vm, 898, &[handle]);
    assert_eq!(
        call(&mut host, &mut vm, 899, &[handle]),
        NativeReturn::Value(4)
    );
    assert_eq!(
        call(&mut host, &mut vm, 518, &[handle, 16]),
        NativeReturn::Value(u32::MAX)
    );
    call(&mut host, &mut vm, 886, &[handle]);
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 885, &[]) else {
        panic!("dictionary handle expected")
    };
    call(&mut host, &mut vm, 516, &[handle]);
}
