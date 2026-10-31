use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{
        CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program, ResourceHost,
        ResourceServices,
    },
    resource::ResourceMode,
};

#[derive(Default)]
struct Services {
    requests: Vec<Vec<u8>>,
}

impl ResourceServices for Services {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.requests.push(name.to_vec());
        match name {
            b"missing" => Ok(None),
            b"bad" => Ok(Some(b"invalid XML".to_vec())),
            b"error" => Err(Error::new(0, "合成资源读取错误")),
            _ => Ok(Some(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document>
                 <value>+0x10tail</value><empty></empty><text>資料%d[G0]&amp;</text>
                 </document>"
                    .as_bytes()
                    .to_vec(),
            )),
        }
    }
}

fn call(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}

#[test]
fn xml_handles_keep_raw_fields_null_shortcuts_and_independent_lifetimes() {
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 256], 64).unwrap(),
        vec![0x5a; 128],
    )
    .unwrap();
    for (address, bytes) in [
        (0, b"data/%04d[G32]\0".as_slice()),
        (32, 7u32.to_le_bytes().as_slice()),
        (48, b"value\0"),
        (64, b"text\0"),
        (80, b"%s[G48]\0"),
        (96, b"empty\0"),
        (112, b"missing\0"),
    ] {
        vm.write(address, bytes).unwrap();
    }
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut services = Services::default();
    let mut host = ResourceHost::new(&mut core, &mut services);
    let NativeReturn::Value(first) = call(&mut host, &mut vm, 590, &[0]) else {
        panic!()
    };
    let NativeReturn::Value(second) = call(&mut host, &mut vm, 590, &[0]) else {
        panic!()
    };
    assert_ne!(first, 0);
    assert_ne!(first, second);
    assert_eq!(
        host.services.requests,
        [b"data/0007".to_vec(), b"data/0007".to_vec()]
    );
    for (handle, default, path, expected) in [
        (first, 99, 80, 16),
        (second, 99, 96, 0),
        (first, 99, 112, 99),
        (0, 99, u32::MAX, 99),
    ] {
        assert_eq!(
            call(&mut host, &mut vm, 592, &[handle, default, path]),
            NativeReturn::Value(expected)
        );
    }
    vm.write(0x80000000, b"fallback%d[G32]\0").unwrap();
    let before = vm.read(0x80000000, 128).unwrap().to_vec();
    assert_eq!(
        call(&mut host, &mut vm, 593, &[first, 0x80000000, 112]),
        NativeReturn::Continue
    );
    assert_eq!(vm.read(0x80000000, 128).unwrap(), before);
    call(&mut host, &mut vm, 593, &[first, 0x80000000, 64]);
    let mut expected = before;
    let value = "資料%d[G0]&amp;\0".as_bytes();
    expected[..value.len()].copy_from_slice(value);
    assert_eq!(vm.read(0x80000000, 128).unwrap(), expected);
    call(&mut host, &mut vm, 593, &[first, 0x80000000, 96]);
    assert_eq!(vm.read(0x80000000, 1).unwrap(), [0]);
    call(&mut host, &mut vm, 593, &[0, u32::MAX, u32::MAX]);
    call(&mut host, &mut vm, 591, &[0]);
    call(&mut host, &mut vm, 591, &[first]);
    let args: Vec<_> = [first, 99, 80]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(host.call(592, &args, &mut vm).is_err());
    assert_eq!(
        call(&mut host, &mut vm, 592, &[second, 99, 80]),
        NativeReturn::Value(16)
    );
    vm.write(0, b"missing\0").unwrap();
    assert_eq!(call(&mut host, &mut vm, 590, &[0]), NativeReturn::Value(0));
    vm.write(0, b"bad\0").unwrap();
    assert_eq!(call(&mut host, &mut vm, 590, &[0]), NativeReturn::Value(0));
}

#[test]
fn resource_service_error_stops_budgeted_ordinary_execution_after_prior_results() {
    fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
        code.push(opcode);
        for arg in args {
            code.push(3);
            code.extend(arg.to_le_bytes());
        }
    }
    let mut code = Vec::new();
    for (native, target) in [(68, 0), (5, 4)] {
        ins(&mut code, 0x2b, &[native, 0]);
        ins(&mut code, 0x26, &[target]);
    }
    for address in [16, 32] {
        ins(&mut code, 0x25, &[address]);
        ins(&mut code, 0x2b, &[590, 4]);
        ins(&mut code, 0x26, &[8]);
    }
    code.push(0x3b);
    let mut data = vec![0; 64];
    data[16..24].copy_from_slice(b"missing\0");
    data[32..38].copy_from_slice(b"error\0");
    let mut task = OrdinaryTask::new(Program::from_code(&code, data, 64).unwrap(), vec![]).unwrap();
    task.task_id = 42;
    task.clock.origin = 10;
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut services = Services::default();
    let mut host = ResourceHost::new(&mut core, &mut services);
    let mut error = None;
    for _ in 0..32 {
        match task.update(100, 1, &mut host, 1) {
            Ok(false) => {}
            result => {
                error = Some(result.unwrap_err());
                break;
            }
        }
    }
    assert_eq!(error.unwrap().message, "合成资源读取错误");
    assert_eq!(
        task.interpreter.read(0, 12).unwrap(),
        [42u32.to_le_bytes(), 90u32.to_le_bytes(), [0; 4]].concat()
    );
    assert_eq!(services.requests, [b"missing".to_vec(), b"error".to_vec()]);
    assert!(task.interpreter.stack_bytes().is_empty());
}
