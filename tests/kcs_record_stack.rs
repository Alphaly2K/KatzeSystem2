use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn call(host: &mut CoreHost<'_>, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    let bytes: Vec<_> = args.iter().flat_map(|value| value.to_le_bytes()).collect();
    host.call(index, &bytes, vm).unwrap()
}

fn create(host: &mut CoreHost<'_>, vm: &mut Interpreter, stride: u32, growth: u32) -> u32 {
    let NativeReturn::Value(handle) = call(host, vm, 863, &[stride, growth]) else {
        panic!("record stack handle expected")
    };
    handle
}

#[test]
fn record_stacks_keep_byte_records_lifo_copies_and_shared_handle_lifetimes() {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 64], 0).unwrap(),
        vec![0; 32],
    )
    .unwrap();
    let source = create(&mut host, &mut vm, 3, 2);
    let copy = create(&mut host, &mut vm, 1, 1);
    for record in [b"one", b"two", b"tri", b"end"] {
        vm.write(0, record).unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 865, &[source, 0]),
            NativeReturn::Value(1)
        );
    }
    assert_eq!(
        call(&mut host, &mut vm, 865, &[source, u32::MAX]),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 868, &[copy, source]);
    call(&mut host, &mut vm, 868, &[source, source]);
    assert_eq!(
        call(&mut host, &mut vm, 866, &[copy, 0x8000_0020]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 867, &[copy]),
        NativeReturn::Value(4)
    );
    for record in [b"end", b"tri", b"two", b"one"] {
        assert_eq!(
            call(&mut host, &mut vm, 866, &[copy, 0x8000_0001]),
            NativeReturn::Value(1)
        );
        assert_eq!(vm.read(0x8000_0001, 3).unwrap(), record);
    }
    assert_eq!(
        call(&mut host, &mut vm, 866, &[copy, 0]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 867, &[source]),
        NativeReturn::Value(4)
    );
    call(&mut host, &mut vm, 868, &[source, 0]);
    call(&mut host, &mut vm, 864, &[copy]);
    let args = copy.to_le_bytes();
    assert!(host.call(867, &args, &mut vm).is_err());
    let NativeReturn::Value(map) = call(&mut host, &mut vm, 515, &[]) else {
        panic!("dictionary handle expected")
    };
    assert!(map > copy);
    assert!(host.call(864, &map.to_le_bytes(), &mut vm).is_err());
    call(&mut host, &mut vm, 516, &[map]);
    call(&mut host, &mut vm, 864, &[source]);

    let empty_record = create(&mut host, &mut vm, 0, 0);
    assert_eq!(
        call(&mut host, &mut vm, 865, &[empty_record, 0]),
        NativeReturn::Value(1)
    );
    assert_eq!(
        call(&mut host, &mut vm, 866, &[empty_record, 0]),
        NativeReturn::Value(1)
    );
    call(&mut host, &mut vm, 864, &[empty_record]);
    for index in 864..=868 {
        assert_eq!(
            call(&mut host, &mut vm, index, &[0]),
            if matches!(index, 864 | 868) {
                NativeReturn::Continue
            } else {
                NativeReturn::Value(0)
            }
        );
    }
    let oversized: Vec<_> = [u32::MAX, 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(host.call(863, &oversized, &mut vm).is_err());
}
