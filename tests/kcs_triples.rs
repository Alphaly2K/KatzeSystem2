use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, ResourceHost, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::Cell, rc::Rc};

struct Files {
    bytes: Option<Vec<u8>>,
    fail: bool,
    reads: Rc<Cell<usize>>,
}
impl ResourceServices for Files {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"data17");
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.set(self.reads.get() + 1);
        if self.fail {
            return Err(Error::new(0, "合成资源读取失败"));
        }
        Ok(self.bytes.clone())
    }
}

fn call(
    host: &mut impl Host,
    vm: &mut Interpreter,
    index: u32,
    args: &[u32],
) -> Result<NativeReturn> {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
}

#[test]
fn triple_tables_keep_partial_headers_first_definitions_and_resource_lifetimes() {
    let mut state = State::new(0);
    let mut core = CoreHost::new(&mut state);
    let mut files = Files {
        bytes: Some(b"loose\r\n#red(11,22,33)\nfoo\n#red(7,8,9)\nbar\n#next(9,\nbaz\n#other(4,5,6)\nfoo\n/*comment\nend*/s a m e//tail\n#lonely(1,2,3)".to_vec()),
        fail: false,
        reads: Rc::default(),
    };
    let mut host = ResourceHost::new(&mut core, &mut files);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 1024], 0).unwrap(),
        vec![0; 16],
    )
    .unwrap();
    vm.write(0, b"data%d[G64]\0").unwrap();
    vm.write(64, &17u32.to_le_bytes()).unwrap();
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 594, &[0]).unwrap() else {
        panic!()
    };
    assert_ne!(handle, 0);
    for (index, name, values) in [
        (596, b"__UNKNOWN__".as_slice(), [100u32; 3]),
        (596, b"#ReD", [11, 22, 33]),
        (597, b"FOO", [11, 22, 33]),
        (597, b"bar", [11, 22, 33]),
        (596, b"#next", [9, 8, 9]),
        (597, b"same", [4, 5, 6]),
    ] {
        vm.write(128, &[name, b"\0"].concat()).unwrap();
        assert_eq!(
            call(
                &mut host,
                &mut vm,
                index,
                &[handle, 128, 256, 260, 0x80000000]
            )
            .unwrap(),
            NativeReturn::Value(1)
        );
        assert_eq!(
            vm.read(256, 8).unwrap(),
            &values[..2]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
        assert_eq!(vm.read(0x80000000, 4).unwrap(), values[2].to_le_bytes());
    }
    vm.write(128, b"#red\0").unwrap();
    call(&mut host, &mut vm, 596, &[handle, 128, 300, 300, 300]).unwrap();
    assert_eq!(vm.read(300, 4).unwrap(), 33u32.to_le_bytes());
    assert!(
        call(
            &mut host,
            &mut vm,
            596,
            &[handle, 128, 304, 308, 0x8000000f]
        )
        .is_err()
    );
    assert_eq!(
        vm.read(304, 8).unwrap(),
        [11u32.to_le_bytes(), 22u32.to_le_bytes()].concat()
    );
    vm.write(128, b"#lonely\0").unwrap();
    assert_eq!(
        call(
            &mut host,
            &mut vm,
            596,
            &[handle, 128, u32::MAX, u32::MAX, u32::MAX]
        )
        .unwrap(),
        NativeReturn::Value(0)
    );
    for index in [596, 597] {
        assert_eq!(
            call(&mut host, &mut vm, index, &[0]).unwrap(),
            NativeReturn::Value(0)
        );
    }
    host.services.fail = true;
    assert!(call(&mut host, &mut vm, 594, &[0]).is_err());
    host.services.fail = false;
    host.services.bytes = Some(vec![b'x'; 512]);
    assert!(call(&mut host, &mut vm, 594, &[0]).is_err());
    host.services.bytes = Some(Vec::new());
    let NativeReturn::Value(empty) = call(&mut host, &mut vm, 594, &[0]).unwrap() else {
        panic!()
    };
    assert_ne!(empty, 0);
    assert_eq!(
        call(&mut host, &mut vm, 596, &[empty, 128, 256, 260, 264]).unwrap(),
        NativeReturn::Value(0)
    );
    host.services.bytes = None;
    assert_eq!(
        call(&mut host, &mut vm, 594, &[0]).unwrap(),
        NativeReturn::Value(0)
    );
    call(&mut host, &mut vm, 595, &[empty]).unwrap();
    call(&mut host, &mut vm, 595, &[handle]).unwrap();
    assert!(call(&mut host, &mut vm, 596, &[handle, 128, 256, 260, 264]).is_err());
    assert!(call(&mut host, &mut vm, 595, &[handle]).is_err());
    assert_eq!(
        call(&mut host, &mut vm, 595, &[0]).unwrap(),
        NativeReturn::Continue
    );
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn runtime_loads_and_queries_both_table_names_then_rejects_the_released_handle() {
    fn ins(code: &mut Vec<u8>, op: u8, args: &[(u8, u32)]) {
        code.push(op);
        for &(flags, value) in args {
            code.push(flags);
            code.extend(value.to_le_bytes());
        }
    }
    let mut code = vec![];
    ins(&mut code, 0x25, &[(3, 0)]);
    ins(&mut code, 0x2b, &[(3, 594), (3, 4)]);
    ins(&mut code, 0x26, &[(3, 80)]);
    for (index, name, output, result) in [(596, 32, 128, 84), (597, 48, 136, 88)] {
        ins(&mut code, 0x25, &[(0x13, 80)]);
        for value in [name, output, output + 4, 0x80000000 + (output - 128) / 2] {
            ins(&mut code, 0x25, &[(3, value)]);
        }
        ins(&mut code, 0x2b, &[(3, index), (3, 20)]);
        ins(&mut code, 0x26, &[(3, result)]);
    }
    ins(&mut code, 0x2b, &[(3, 65), (3, 0)]);
    ins(&mut code, 0x25, &[(0x13, 80)]);
    ins(&mut code, 0x2b, &[(3, 595), (3, 4)]);
    ins(&mut code, 0x25, &[(0x13, 80)]);
    for value in [32, 128, 132, 0x80000000] {
        ins(&mut code, 0x25, &[(3, value)]);
    }
    ins(&mut code, 0x2b, &[(3, 596), (3, 20)]);
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[..7].copy_from_slice(b"data17\0");
    data[32..37].copy_from_slice(b"#red\0");
    data[48..52].copy_from_slice(b"foo\0");
    let program = Program::from_code(&code, data, 64).unwrap();
    for budget in [1, 4096] {
        let mut factory = KcsFactory::new(Loader(program.clone()));
        factory.external = katze_system2::memory::SharedMemory::new(vec![0; 8]).unwrap();
        let mut runtime = Runtime::new(
            0,
            kind::BASE,
            Box::new(BaseObject::new()),
            State::new(0),
            factory,
        );
        let reads = Rc::default();
        runtime.set_resources(Files {
            bytes: Some(b"#red(11,22,33)\nfoo".to_vec()),
            fail: false,
            reads: Rc::clone(&reads),
        });
        runtime.set_budget(budget, 10000);
        let task = runtime.create(0, kind::ORDINARY, b"table", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(84, 8).unwrap(), [1u32.to_le_bytes(); 2].concat());
        assert_eq!(
            vm.read(128, 16).unwrap(),
            [11u32, 22, 11, 22]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            vm.read(0x80000000, 8).unwrap(),
            [33u32.to_le_bytes(); 2].concat()
        );
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(reads.get(), 1);
        assert!(
            runtime
                .update(1, 1)
                .unwrap_err()
                .message
                .contains("三整数分组表句柄无效")
        );
    }
}
