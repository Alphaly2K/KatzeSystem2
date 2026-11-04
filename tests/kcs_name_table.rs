use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, ResourceHost, ResourceServices},
    resource::ResourceMode,
};

struct Files;

impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"fes");
        Ok(name.to_vec())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(match name {
            b"names" => Some(
                b"#header\r\n1\t\"\x82\xa0\"tail\tV\tC\talias\n2\tBob\t\"V\nW\"\t\talias\n"
                    .to_vec(),
            ),
            b"empty" => Some(vec![]),
            _ => None,
        })
    }
}

fn call(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    let bytes: Vec<_> = args.iter().flat_map(|value| value.to_le_bytes()).collect();
    host.call(index, &bytes, vm).unwrap()
}

#[test]
fn name_tables_preserve_rows_aliases_encoding_and_output_shortcuts() {
    for unicode in [0, 1] {
        let mut state = State::new(0);
        let mut core = CoreHost::new(&mut state);
        core.unicode_text = unicode;
        let mut inner = &mut core;
        let mut files = Files;
        let mut host = ResourceHost::new(&mut inner, &mut files);
        let mut vm = Interpreter::new(
            Program::from_code(&[], vec![0; 128], 0).unwrap(),
            vec![0; 32],
        )
        .unwrap();
        for name in [b"missing\0".as_slice(), b"empty\0"] {
            vm.write(0, name).unwrap();
            assert_eq!(call(&mut host, &mut vm, 633, &[0]), NativeReturn::Value(0));
        }
        vm.write(0, b"names\0").unwrap();
        let NativeReturn::Value(handle) = call(&mut host, &mut vm, 633, &[0]) else {
            panic!("name table handle expected")
        };
        assert_eq!(handle, 1);
        vm.write(16, b"alias\0").unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 635, &[handle, 16]),
            NativeReturn::Value(0)
        );
        assert_eq!(
            call(&mut host, &mut vm, 636, &[handle, 1]),
            NativeReturn::Value(2)
        );
        assert_eq!(
            call(&mut host, &mut vm, 637, &[handle, 0, 0]),
            NativeReturn::Value(1)
        );
        assert_eq!(
            vm.read_string(0).unwrap(),
            if unicode == 0 {
                b"\x82\xa0".as_slice()
            } else {
                "あ".as_bytes()
            }
        );
        assert_eq!(
            call(&mut host, &mut vm, 638, &[handle, 1, 0, 0x8000_0001]),
            NativeReturn::Value(1)
        );
        assert_eq!(vm.read_string(0x8000_0001).unwrap(), b"VW");
        let invalid_output = [handle, 0, u32::MAX]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(host.call(637, &invalid_output, &mut vm).is_err());
        assert_eq!(
            call(&mut host, &mut vm, 638, &[handle, 0, 0, u32::MAX]),
            NativeReturn::Value(1)
        );
        assert_eq!(
            call(&mut host, &mut vm, 638, &[handle, 0, 2, 0]),
            NativeReturn::Value(0)
        );
        assert_eq!(
            call(&mut host, &mut vm, 636, &[handle, u32::MAX]),
            NativeReturn::Value(u32::MAX)
        );
        let NativeReturn::Value(map) = call(&mut host, &mut vm, 515, &[]) else {
            panic!()
        };
        assert!(map > handle);
        assert!(host.call(634, &map.to_le_bytes(), &mut vm).is_err());
        call(&mut host, &mut vm, 516, &[map]);
        call(&mut host, &mut vm, 634, &[handle]);
        assert!(
            host.call(
                636,
                &[handle.to_le_bytes(), 0u32.to_le_bytes()].concat(),
                &mut vm
            )
            .is_err()
        );
        for index in 634..=638 {
            assert_eq!(
                call(&mut host, &mut vm, index, &[0]),
                match index {
                    634 => NativeReturn::Continue,
                    635 | 636 => NativeReturn::Value(u32::MAX),
                    _ => NativeReturn::Value(0),
                }
            );
        }
    }
}

#[test]
fn runtime_name_tables_survive_the_creator_and_resource_service_replacement() {
    use katze_system2::runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind};

    struct Loader;
    impl ProgramLoader for Loader {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            let mut instruction = |opcode, args: &[(u8, u32)]| {
                code.push(opcode);
                for &(flags, value) in args {
                    code.push(flags);
                    code.extend(value.to_le_bytes());
                }
            };
            if name == b"create" {
                instruction(0x25, &[(3, 16)]);
                instruction(0x2b, &[(3, 633), (3, 4)]);
                instruction(0x26, &[(3, 0)]);
            } else {
                for arg in [(0x13, 0), (3, 0), (3, 32)] {
                    instruction(0x25, &[arg]);
                }
                instruction(0x2b, &[(3, 637), (3, 12)]);
                instruction(0x26, &[(3, 4)]);
                instruction(0x25, &[(0x13, 0)]);
                instruction(0x2b, &[(3, 634), (3, 4)]);
            }
            code.push(0x3b);
            let mut data = vec![0; 64];
            data[16..22].copy_from_slice(b"names\0");
            Ok(Some(Program::from_code(&code, data, 32)?))
        }
    }

    struct NoReads;
    impl ResourceServices for NoReads {
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("queries must use the retained name table")
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
        runtime.host_mut().unicode_text = 1;
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
        runtime.host_mut().unicode_text = 0;
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
        assert_eq!(vm.read_string(32).unwrap(), "あ".as_bytes());
        assert_eq!(vm.read(4, 4).unwrap(), 1u32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
    }
}
