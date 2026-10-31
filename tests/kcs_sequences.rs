use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn call(host: &mut CoreHost<'_>, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    let args: Vec<_> = args.iter().flat_map(|value| value.to_le_bytes()).collect();
    host.call(index, &args, vm).unwrap()
}

fn text(
    host: &mut CoreHost<'_>,
    vm: &mut Interpreter,
    index: u32,
    handle: u32,
    source: &[u8],
) -> NativeReturn {
    vm.write(256, source).unwrap();
    vm.write(256 + source.len() as u32, &[0]).unwrap();
    call(host, vm, index, &[handle, 256])
}

fn create(host: &mut CoreHost<'_>, vm: &mut Interpreter) -> u32 {
    let NativeReturn::Value(handle) = call(host, vm, 419, &[]) else {
        panic!("sequence handle expected")
    };
    assert_ne!(handle, 0);
    handle
}

#[test]
fn sequence_maps_keep_byte_components_terminal_edges_and_separate_lifetimes() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 1024], 32).unwrap(), vec![]).unwrap();
    let first = create(&mut host, &mut vm);
    let second = create(&mut host, &mut vm);
    assert_ne!(first, second);
    vm.write(16, b"a,b,1\0").unwrap();
    for source in [
        b"%s[G16]".as_slice(),
        b",a,,b,,1,,2,",
        b"a,b,\xff",
        b"a,b,\x80",
    ] {
        assert_eq!(
            text(&mut host, &mut vm, 422, first, source),
            NativeReturn::Continue
        );
    }
    for (source, found) in [
        (b"a,b,10".as_slice(), true),
        (b"a,b,1,2", true),
        (b"a,b,1,2,3", false),
        (b"a,b,1,\xff", true),
        (b"a,b,1,\xff,1", false),
        (b"a,b,\x80", true),
        (b"a,b,\xfe", false),
        (b"a,b", true),
        (b"A,b,1", false),
        (b"a,b, 1", false),
        (b",,,", false),
    ] {
        assert_eq!(
            text(&mut host, &mut vm, 423, first, source),
            NativeReturn::Value(u32::from(found))
        );
    }
    assert_eq!(
        text(&mut host, &mut vm, 423, second, b"a,b,1"),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 421, &[first]);
    assert_eq!(
        text(&mut host, &mut vm, 423, first, b"a,b,1"),
        NativeReturn::Value(0)
    );
    text(&mut host, &mut vm, 422, first, b"a,b,1");
    assert_eq!(
        text(&mut host, &mut vm, 423, first, b"a,b"),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 420, &[first]);
    assert!(
        host.call(
            423,
            &[first, 256]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            &mut vm
        )
        .is_err()
    );
    assert_eq!(
        call(&mut host, &mut vm, 422, &[0, u32::MAX]),
        NativeReturn::Continue
    );
    assert_eq!(
        call(&mut host, &mut vm, 423, &[0, u32::MAX]),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 420, &[0]);
    call(&mut host, &mut vm, 421, &[0]);
    let fresh = create(&mut host, &mut vm);
    assert_ne!(fresh, first);
    assert_eq!(
        text(&mut host, &mut vm, 423, fresh, b"a,b,1"),
        NativeReturn::Value(0)
    );
}

#[test]
fn sequence_name_modes_add_only_the_first_matching_variant() {
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 1024], 32).unwrap(), vec![]).unwrap();
    for mode in [0, 1, 2, u32::MAX] {
        host.sequence_name_mode = mode;
        let handle = create(&mut host, &mut vm);
        for source in [b"gaL_xl,01,1".as_slice(), b"l,a,1"] {
            text(&mut host, &mut vm, 422, handle, source);
        }
        for (source, found) in [
            (b"gaL_xl,01,1".as_slice(), true),
            (b"ga_xl,01,1", mode == 0),
            (b"gam_xl,01,1", mode == 1),
            (b"gaL_xm,01,1", false),
            (b"l,a,1", true),
            (b"m,a,1", false),
        ] {
            assert_eq!(
                text(&mut host, &mut vm, 423, handle, source),
                NativeReturn::Value(u32::from(found))
            );
        }
    }
}
