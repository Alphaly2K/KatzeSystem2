use katze_system2::{
    Result,
    expression::State,
    host::resources::{ResourceMode, ResourceServices},
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, ResourceHost},
};

struct Files;
impl ResourceServices for Files {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(match name {
            b"table" => Some(b"ignored before\n #Sec \r\n key\t one  two //tail\rkey three\nkey a/*/b\nkey begin/*comment\nignored*/suffix\n#Other\nKey elsewhere\n#Sec\nkey last\nempty\n".to_vec()),
            b"empty" => Some(vec![]),
            _ => None,
        })
    }
}

fn call(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}

#[test]
fn sections_keep_exact_names_repeated_keys_normalization_and_output_gates() {
    let mut state = State::new(0);
    let mut core = CoreHost::new(&mut state);
    let mut inner = &mut core;
    let mut files = Files;
    let mut host = ResourceHost::new(&mut inner, &mut files);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 128], 0).unwrap(),
        vec![0; 32],
    )
    .unwrap();
    vm.write(0, b"missing\0").unwrap();
    assert_eq!(call(&mut host, &mut vm, 683, &[0]), NativeReturn::Value(0));
    vm.write(0, b"empty\0").unwrap();
    let NativeReturn::Value(empty) = call(&mut host, &mut vm, 683, &[0]) else {
        panic!()
    };
    assert_ne!(empty, 0);
    assert_eq!(
        call(&mut host, &mut vm, 685, &[empty, 0, 0, 0, 0]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 684, &[empty]),
        NativeReturn::Continue
    );
    vm.write(0, b"table\0").unwrap();
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 683, &[0]) else {
        panic!()
    };
    assert!(handle > empty);
    vm.write(64, b"#Sec\0").unwrap();
    vm.write(96, b"key\0").unwrap();
    for (occurrence, expected) in [b"one two".as_slice(), b"three", b"ab", b"begin", b"last"]
        .into_iter()
        .enumerate()
    {
        for output in [0, 0x8000_0001] {
            assert_eq!(
                call(
                    &mut host,
                    &mut vm,
                    685,
                    &[handle, 64, 96, occurrence as u32, output]
                ),
                NativeReturn::Value(1)
            );
            assert_eq!(vm.read_string(output).unwrap(), expected);
        }
    }
    let before = vm.read(0, 16).unwrap().to_vec();
    for occurrence in [5, u32::MAX] {
        assert_eq!(
            call(&mut host, &mut vm, 685, &[handle, 64, 96, occurrence, 0]),
            NativeReturn::Value(0)
        );
    }
    assert_eq!(
        call(&mut host, &mut vm, 685, &[handle, 64, 96, 0, u32::MAX]),
        NativeReturn::Value(0)
    );
    vm.write(64, b"#sec\0").unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 685, &[handle, 64, 96, 0, 0]),
        NativeReturn::Value(0)
    );
    assert_eq!(vm.read(0, 16).unwrap(), before);
    vm.write(64, b"#Sec\0").unwrap();
    vm.write(96, b"suffix\0").unwrap();
    vm.write(0, b"unchanged\0").unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 685, &[handle, 64, 96, 0, 0]),
        NativeReturn::Value(1)
    );
    assert_eq!(vm.read(0, 4).unwrap(), b"\0nch");
    vm.write(96, b"key\0").unwrap();
    let invalid: Vec<_> = [handle, 64, 96, 0, 127]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(host.call(685, &invalid, &mut vm).is_err());
    assert!(host.call(683, &[], &mut vm).is_err());
    assert!(host.call(685, &handle.to_le_bytes(), &mut vm).is_err());
    let NativeReturn::Value(other) = call(&mut host, &mut vm, 515, &[]) else {
        panic!()
    };
    assert!(other > handle);
    assert!(host.call(684, &other.to_le_bytes(), &mut vm).is_err());
    call(&mut host, &mut vm, 516, &[other]);
    assert_eq!(call(&mut host, &mut vm, 684, &[0]), NativeReturn::Continue);
    assert_eq!(call(&mut host, &mut vm, 685, &[0]), NativeReturn::Value(0));
    call(&mut host, &mut vm, 684, &[handle]);
    assert!(host.call(685, &invalid, &mut vm).is_err());
    assert!(host.call(684, &handle.to_le_bytes(), &mut vm).is_err());
}

#[test]
fn runtime_sections_survive_the_creator_and_resource_service_replacement() {
    use katze_system2::runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind};
    struct Loader;
    impl ProgramLoader for Loader {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            let mut ins = |opcode, args: &[(u8, u32)]| {
                code.push(opcode);
                for &(flags, value) in args {
                    code.push(flags);
                    code.extend(value.to_le_bytes());
                }
            };
            if name == b"create" {
                ins(0x25, &[(3, 16)]);
                ins(0x2b, &[(3, 683), (3, 4)]);
                ins(0x26, &[(3, 0)]);
            } else {
                for arg in [(0x13, 0), (3, 32), (3, 48), (3, 1), (3, 64)] {
                    ins(0x25, &[arg]);
                }
                ins(0x2b, &[(3, 685), (3, 20)]);
                ins(0x26, &[(3, 4)]);
                ins(0x25, &[(0x13, 0)]);
                ins(0x2b, &[(3, 684), (3, 4)]);
            }
            code.push(0x3b);
            let mut data = vec![0; 96];
            data[16..22].copy_from_slice(b"table\0");
            data[32..37].copy_from_slice(b"#Sec\0");
            data[48..52].copy_from_slice(b"key\0");
            Ok(Some(Program::from_code(&code, data, 32)?))
        }
    }
    struct NoReads;
    impl ResourceServices for NoReads {
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("queries must use the retained section table")
        }
    }
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.set_resources(Files);
        let creator = runtime.create(0, kind::ORDINARY, b"create", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let handle = runtime
            .object(creator)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(0, 4)
            .unwrap()
            .to_vec();
        runtime.destroy(creator).unwrap();
        runtime.set_resources(NoReads);
        let reader = runtime.create(0, kind::ORDINARY, b"read", b"").unwrap();
        runtime
            .object_mut(reader)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(0, &handle)
            .unwrap();
        runtime.update(1, 0).unwrap();
        let vm = runtime.object(reader).unwrap().interpreter().unwrap();
        assert_eq!(vm.read_string(64).unwrap(), b"three");
        assert_eq!(vm.read(4, 4).unwrap(), 1u32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
    }
}
