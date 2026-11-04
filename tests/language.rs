use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    language::LanguageState,
};

#[test]
fn language_native_preserves_mode_fallback_cascade_and_atomic_vm_writes() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    host.languages = LanguageState {
        prefixes: vec![b"aa".to_vec(), b"%LANG_M%".to_vec(), b"zz".to_vec()],
        default_index: 2,
        primary_index: 1,
        message_index: u32::MAX,
    };
    assert_eq!(
        host.languages
            .expand_image(b"%LANG%/%LANG_M%/%lang%\0ignored")
            .unwrap(),
        b"zz/zz/%lang%"
    );
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xcc; 128], 0).unwrap(),
        vec![0xdd; 64],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, mode: u32, address: u32| {
        host.call(
            1062,
            &[mode.to_le_bytes(), address.to_le_bytes()].concat(),
            vm,
        )
    };
    for address in [0, 0x80000000] {
        vm.write(address, b"%LANG%:%LANG%:%LANG_M%\0tail").unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 0, address).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(
            vm.read_string(address).unwrap(),
            b"%LANG_M%:%LANG_M%:%LANG_M%"
        );
        call(&mut host, &mut vm, 1, address).unwrap();
        assert_eq!(vm.read_string(address).unwrap(), b"zz:zz:zz");
    }
    for mode in [2, u32::MAX] {
        assert_eq!(
            call(&mut host, &mut vm, mode, u32::MAX).unwrap(),
            NativeReturn::Continue
        );
    }
    host.languages.default_index = u32::MAX;
    assert_eq!(host.languages.expand(b"%LANG_M%", 1).unwrap(), b"%LANG_M%");
    host.languages.primary_index = 0;
    host.languages.prefixes[0] = vec![b'x'; 32];
    vm.write(112, b"%LANG%\0").unwrap();
    let before = vm.read(112, 16).unwrap().to_vec();
    assert!(call(&mut host, &mut vm, 0, 112).is_err());
    assert_eq!(vm.read(112, 16).unwrap(), before);
    host.languages.prefixes[0] = vec![b'x'; 255];
    assert!(host.languages.expand(&b"%LANG%".repeat(9), 0).is_err());
    assert!(host.call(1062, &[0; 4], &mut vm).is_err());
}

#[test]
fn prefix_query_keeps_cp932_fallback_empty_values_and_ordered_overlapping_outputs() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    host.languages = LanguageState {
        prefixes: vec![
            b"abc\0tail".to_vec(),
            b"XY".to_vec(),
            vec![0x93, 0xfa, 0x96, 0x7b],
        ],
        default_index: 2,
        primary_index: 0,
        message_index: 1,
    };
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xcc; 64], 0).unwrap(),
        vec![0xdd; 64],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, first: u32, second: u32| {
        host.call(
            1036,
            &[first.to_le_bytes(), second.to_le_bytes()].concat(),
            vm,
        )
    };
    for (first, second, expected) in [
        (0, 0, b"XY\0\0".as_slice()),
        (0, 1, b"aXY\0"),
        (1, 0, b"XY\0c\0"),
    ] {
        vm.write(0, &[0xcc; 64]).unwrap();
        assert_eq!(
            call(&mut host, &mut vm, first, second).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(vm.read(0, expected.len()).unwrap(), expected);
    }
    host.languages.primary_index = u32::MAX;
    for unicode in [0, 1, u32::MAX] {
        host.unicode_text = unicode;
        call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
        assert_eq!(vm.read_string(0).unwrap(), [0x93, 0xfa, 0x96, 0x7b]);
        assert_eq!(vm.read_string(0x8000_0000).unwrap(), b"XY");
    }
    host.languages.prefixes[1].clear();
    call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
    assert!(vm.read_string(0x8000_0000).unwrap().is_empty());
    host.languages.default_index = u32::MAX;
    call(&mut host, &mut vm, 0, 0x8000_0000).unwrap();
    assert!(vm.read_string(0).unwrap().is_empty());
    vm.write(0, b"keep\0").unwrap();
    assert!(call(&mut host, &mut vm, 0, u32::MAX).is_err());
    assert_eq!(vm.read_string(0).unwrap(), b"keep");
    assert!(host.call(1036, &[0; 4], &mut vm).is_err());
}
