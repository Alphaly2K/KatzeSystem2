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
        (900, vec![0, u32::MAX], NativeReturn::Continue),
        (900, vec![u32::MAX, 0], NativeReturn::Continue),
        (901, vec![0, u32::MAX], NativeReturn::Continue),
        (901, vec![u32::MAX, 0], NativeReturn::Continue),
        (902, vec![u32::MAX, 0, u32::MAX], NativeReturn::Value(0)),
        (895, vec![0], NativeReturn::Continue),
        (676, vec![0], NativeReturn::Continue),
        (888, vec![0, u32::MAX], NativeReturn::Value(0)),
        (889, vec![0], NativeReturn::Continue),
        (890, vec![0], NativeReturn::Value(0)),
        (893, vec![u32::MAX, 0, u32::MAX], NativeReturn::Value(0)),
    ] {
        assert_eq!(call(&mut host, &mut vm, index, &args), expected);
    }
}

#[test]
fn named_values_share_dictionary_handles_and_keep_formatted_updates_and_sorted_keys() {
    for (new, release, query, clear, size, key) in [
        (894, 886, 897, 898, 899, 902),
        (675, 676, 888, 889, 890, 893),
        (885, 895, 897, 889, 899, 893),
        (515, 516, 888, 898, 890, 902),
    ] {
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
        let NativeReturn::Value(handle) = call(&mut host, &mut vm, new, &[u32::MAX]) else {
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
            call(&mut host, &mut vm, query, &[handle, 16]),
            NativeReturn::Value(0x80000001)
        );
        assert_eq!(
            call(&mut host, &mut vm, query, &[handle, 96]),
            NativeReturn::Value(0)
        );
        assert_eq!(
            call(&mut host, &mut vm, size, &[handle]),
            NativeReturn::Value(40)
        );
        for (index, expected_key) in [b"%d[G0]".as_slice(), b"17", b"ABC", b"abc"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                call(&mut host, &mut vm, key, &[handle, index as u32, 0x80000000]),
                NativeReturn::Value(1)
            );
            assert_eq!(vm.read_string(0x80000000).unwrap(), expected_key);
        }
        let before = vm.read(0x80000000, 64).unwrap().to_vec();
        assert_eq!(
            call(&mut host, &mut vm, key, &[handle, u32::MAX, 0x80000000]),
            NativeReturn::Value(0)
        );
        assert_eq!(vm.read(0x80000000, 64).unwrap(), before);
        let NativeReturn::Value(raw) = call(&mut host, &mut vm, 833, &[64]) else {
            panic!()
        };
        host.raw_buffer_mut(raw, 64).unwrap().fill(0xcc);
        call(&mut host, &mut vm, 900, &[handle, raw + 1]);
        let mut expected = vec![0xcc; 40];
        expected[..4].copy_from_slice(&4u32.to_le_bytes());
        expected[4..11].copy_from_slice(b"%d[G0]\0");
        expected[12..16].copy_from_slice(&7u32.to_le_bytes());
        expected[16..19].copy_from_slice(b"17\0");
        expected[20..24].copy_from_slice(&0x80000001u32.to_le_bytes());
        expected[24..28].copy_from_slice(b"ABC\0");
        expected[28..32].copy_from_slice(&9u32.to_le_bytes());
        expected[32..36].copy_from_slice(b"abc\0");
        expected[36..40].copy_from_slice(&11u32.to_le_bytes());
        assert_eq!(host.raw_buffer(raw + 1, 40).unwrap(), expected);
        assert_eq!(host.raw_buffer(raw, 1).unwrap(), [0xcc]);
        host.raw_buffer_mut(raw + 1, 4)
            .unwrap()
            .copy_from_slice(&5u32.to_le_bytes());
        host.raw_buffer_mut(raw + 41, 8)
            .unwrap()
            .copy_from_slice(b"abc\0\x99\0\0\0");
        let target = create(&mut host, &mut vm);
        call(&mut host, &mut vm, 517, &[target, 48, 55]);
        call(&mut host, &mut vm, 517, &[target, 96, 33]);
        call(&mut host, &mut vm, 901, &[target, raw + 1]);
        for (key, value) in [(48, 55), (96, 33), (80, 11)] {
            assert_eq!(
                call(&mut host, &mut vm, 518, &[target, key]),
                NativeReturn::Value(value)
            );
        }
        let bad: Vec<_> = [target, raw + 63]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(host.call(901, &bad, &mut vm).is_err());
        let short: Vec<_> = [handle, raw + 63]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(host.call(900, &short, &mut vm).is_err());
        call(&mut host, &mut vm, 834, &[raw]);
        assert_eq!(
            call(&mut host, &mut vm, 518, &[target, 80]),
            NativeReturn::Value(11)
        );
        call(&mut host, &mut vm, 516, &[target]);
        call(&mut host, &mut vm, clear, &[handle]);
        assert_eq!(
            call(&mut host, &mut vm, size, &[handle]),
            NativeReturn::Value(4)
        );
        assert_eq!(
            call(&mut host, &mut vm, 518, &[handle, 16]),
            NativeReturn::Value(u32::MAX)
        );
        call(&mut host, &mut vm, release, &[handle]);
        let NativeReturn::Value(handle) = call(&mut host, &mut vm, 885, &[]) else {
            panic!("dictionary handle expected")
        };
        call(&mut host, &mut vm, 516, &[handle]);
    }
}
