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

#[test]
fn sequence_container_format_keeps_full_keys_first_records_and_bounded_tree_reads() {
    fn root(name: &[u8], count: u32) -> Vec<u8> {
        let mut bytes = vec![0; 68];
        bytes[..4].copy_from_slice(&count.to_le_bytes());
        bytes[4..4 + name.len()].copy_from_slice(name);
        bytes
    }
    fn edge(bytes: &mut Vec<u8>, count: u32, key: i32) {
        bytes.extend(count.to_le_bytes());
        bytes.extend(key.to_le_bytes());
    }
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 8192], 32).unwrap(), vec![]).unwrap();
    let map = create(&mut host, &mut vm);
    let NativeReturn::Value(container) = call(&mut host, &mut vm, 238, &[77]) else {
        panic!()
    };
    let mut expected = root(b"a_b", 3);
    edge(&mut expected, 0, -128);
    edge(&mut expected, 1, 49);
    edge(&mut expected, 0, -1);
    edge(&mut expected, 0, 256);
    let mut source = expected.clone();
    source[8..68].fill(0xa5);
    source.extend(root(b"a_b", 0));
    source.extend(root(b"ignored", 0));
    vm.write(1024, &source).unwrap();
    call(
        &mut host,
        &mut vm,
        244,
        &[container, 17, source.len() as u32, 1024],
    );
    for _ in 0..2 {
        assert_eq!(
            call(&mut host, &mut vm, 425, &[container, 17, map]),
            NativeReturn::Value(1)
        );
        for (query, found) in [
            (b"a,b,1".as_slice(), 1),
            (b"a,b,\x80", 1),
            (b"a,b,\xff", 0),
            (b"ignored", 0),
        ] {
            assert_eq!(
                text(&mut host, &mut vm, 423, map, query),
                NativeReturn::Value(found)
            );
        }
        call(&mut host, &mut vm, 424, &[container, 30, map]);
        assert_eq!(
            host.data_container(container).unwrap().record(30).unwrap(),
            Some(expected.as_slice())
        );
        call(&mut host, &mut vm, 421, &[map]);
    }
    let used = host.data_container(container).unwrap().used_len();
    call(&mut host, &mut vm, 424, &[container, 30, map]);
    assert_eq!(host.data_container(container).unwrap().used_len(), used + 8);
    for args in [vec![0], vec![0, u32::MAX, u32::MAX], vec![container, 17, 0]] {
        for index in [424, 425] {
            assert_eq!(
                call(&mut host, &mut vm, index, &args),
                NativeReturn::Value(0)
            );
        }
    }
    assert_eq!(
        call(&mut host, &mut vm, 425, &[container, 999, u32::MAX]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 424, &[u32::MAX, 17, 0]),
        NativeReturn::Value(0)
    );
    let mut unterminated = root(b"a_b", 0);
    unterminated[4..].fill(b'x');
    let mut duplicate = root(b"a_b", 2);
    edge(&mut duplicate, 0, 49);
    edge(&mut duplicate, 0, 49);
    let mut too_deep = root(b"a_b", 1);
    for _ in 0..257 {
        edge(&mut too_deep, 1, 49);
    }
    edge(&mut too_deep, 0, -1);
    for (index, bytes) in [
        vec![0; 67],
        unterminated,
        root(b"a_b", 1),
        root(b"a_b", i32::MAX as u32),
        duplicate,
        too_deep,
    ]
    .into_iter()
    .enumerate()
    {
        let id = 100 + index as u32;
        vm.write(1024, &bytes).unwrap();
        call(
            &mut host,
            &mut vm,
            244,
            &[container, id, bytes.len() as u32, 1024],
        );
        let args: Vec<_> = [container, id, map]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(host.call(425, &args, &mut vm).is_err());
        call(&mut host, &mut vm, 425, &[container, 30, map]);
    }
    call(&mut host, &mut vm, 246, &[container]);
    assert_eq!(
        text(&mut host, &mut vm, 423, map, b"a,b,1"),
        NativeReturn::Value(1)
    );
}
