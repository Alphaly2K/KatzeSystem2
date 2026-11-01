use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{
        CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program, ResourceHost,
        ResourceServices,
    },
    resource::{ArchiveConfig, ResourceMode},
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
fn resource_wrappers_share_handles_with_collections_and_external_images() {
    struct Files;
    impl ResourceServices for Files {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }
        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(match name {
                b"xml" => Some(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><document/>".to_vec()),
                b"raw" => Some(vec![7]),
                b"bad" => Some(b"invalid XML".to_vec()),
                b"empty" => Some(vec![]),
                _ => None,
            })
        }
    }
    fn value(result: NativeReturn) -> u32 {
        let NativeReturn::Value(handle) = result else {
            panic!()
        };
        handle
    }
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 32], 16).unwrap(), vec![]).unwrap();
    let mut numbers = State::new(0);
    let mut core = CoreHost::new(&mut numbers);
    let mut inner = &mut core;
    let mut services = Files;
    let mut host = ResourceHost::new(&mut inner, &mut services);
    let mut images =
        katze_system2::graphics::Images::with_handles(host.handle_allocator().unwrap());
    let mut handles = vec![value(call(&mut host, &mut vm, 419, &[]))];
    assert!(images.create(0, [u32::MAX, 2]).is_err());
    for name in [b"missing\0".as_slice(), b"bad\0"] {
        vm.write(0, name).unwrap();
        assert_eq!(call(&mut host, &mut vm, 590, &[0]), NativeReturn::Value(0));
    }
    for name in [b"missing\0".as_slice(), b"empty\0"] {
        vm.write(0, name).unwrap();
        assert_eq!(call(&mut host, &mut vm, 365, &[0]), NativeReturn::Value(0));
    }
    handles.push(images.create(0, [1, 1]).unwrap());
    for (native, name) in [(365, b"raw\0".as_slice()), (590, b"xml\0")] {
        vm.write(0, name).unwrap();
        handles.push(value(call(&mut host, &mut vm, native, &[0])));
    }
    handles.push(value(call(&mut host, &mut vm, 515, &[])));
    assert_eq!(handles, [1, 2, 3, 4, 5]);
    images.release(handles[1]).unwrap();
    call(&mut host, &mut vm, 420, &[handles[0]]);
    assert_eq!(value(call(&mut host, &mut vm, 419, &[])), 6);
    assert_eq!(images.create(0, [0, 0]).unwrap(), 7);
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

#[test]
fn image_data_uses_category_resolution_and_keeps_each_load_alive_until_release() {
    struct Images {
        config: ArchiveConfig,
        requests: Vec<Vec<u8>>,
    }
    impl ResourceServices for Images {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            self.config.resolve(category, name)
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            self.requests.push(name.to_vec());
            Ok(match name {
                b"images.int/empty.hg3" => Some(vec![]),
                b"images.int/missing.hg3" => None,
                _ => Some(b"raw image bytes".to_vec()),
            })
        }
    }
    let mut config = ArchiveConfig::default();
    config.entries[0].name = b"image".to_vec();
    config.entries[0].category.prefix = b"images.int".to_vec();
    config.entries[0].category.extension = b"hg3".to_vec();
    let mut services = Images {
        config,
        requests: vec![],
    };
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 128], 64).unwrap(), vec![]).unwrap();
    vm.write(0, b"art%04d[G32]\0").unwrap();
    vm.write(32, &7u32.to_le_bytes()).unwrap();
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut host = ResourceHost::new(&mut core, &mut services);
    let NativeReturn::Value(first) = call(&mut host, &mut vm, 365, &[0]) else {
        panic!()
    };
    vm.write(0, b"folder/item.hg3\0").unwrap();
    let NativeReturn::Value(second) = call(&mut host, &mut vm, 365, &[0]) else {
        panic!()
    };
    assert_ne!(first, 0);
    assert_ne!(first, second);
    assert_eq!(host.image_data(first).unwrap(), b"raw image bytes");
    assert_eq!(call(&mut host, &mut vm, 366, &[0]), NativeReturn::Continue);
    assert_eq!(
        call(&mut host, &mut vm, 366, &[first]),
        NativeReturn::Continue
    );
    assert!(host.image_data(first).is_err());
    assert_eq!(host.image_data(second).unwrap(), b"raw image bytes");
    vm.write(64, &[0x5a; 40]).unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 368, &[second, 0, 64]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        vm.read(64, 40).unwrap(),
        [[0; 36].as_slice(), &[0x5a; 4]].concat()
    );
    for name in [b"empty\0".as_slice(), b"missing\0"] {
        vm.write(0, name).unwrap();
        assert_eq!(call(&mut host, &mut vm, 365, &[0]), NativeReturn::Value(0));
    }
    assert_eq!(
        services.requests,
        [
            b"images.int/art0007.hg3".to_vec(),
            b"folder/item.hg3".to_vec(),
            b"images.int/empty.hg3".to_vec(),
            b"images.int/missing.hg3".to_vec(),
        ]
    );
}

#[test]
fn image_metadata_queries_select_first_frame_and_write_only_the_nine_fields() {
    struct Bytes(Vec<u8>);
    impl ResourceServices for Bytes {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(Some(self.0.clone()))
        }
    }
    let mut image = b"HG-3".to_vec();
    image.extend([12u32, 0x300].into_iter().flat_map(u32::to_le_bytes));
    for (i, id) in [7u32, 7, u32::MAX].into_iter().enumerate() {
        image.extend((if i == 2 { 0u32 } else { 64 }).to_le_bytes());
        image.extend(id.to_le_bytes());
        image.extend(b"stdinfo\0");
        image.extend([0u32, 40].into_iter().flat_map(u32::to_le_bytes));
        let i = i as u32;
        image.extend(
            [
                31 + i,
                47 + i,
                if i == 2 { 0x8000 } else { 32 },
                (-3i32).wrapping_sub(i as i32) as u32,
                11 + i,
                640 + i,
                480 + i,
                0,
                100 + i,
                (-21i32).wrapping_sub(i as i32) as u32,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
    }
    let mut services = Bytes(image);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 64], 64).unwrap(),
        vec![0x5a; 64],
    )
    .unwrap();
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut host = ResourceHost::new(&mut core, &mut services);
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 365, &[0]) else {
        panic!()
    };
    for (frame, expected) in [
        (7, [-3i32, 11, 31, 47, 640, 480, 4, 100, -21]),
        (u32::MAX, [-5, 13, 33, 49, 642, 482, -4096, 102, -23]),
    ] {
        assert_eq!(
            call(&mut host, &mut vm, 368, &[handle, frame, 0x80000008]),
            NativeReturn::Value(1)
        );
        let expected: Vec<_> = expected.into_iter().flat_map(i32::to_le_bytes).collect();
        assert_eq!(vm.read(0x80000008, 36).unwrap(), expected);
    }
    let before = vm.read(0x80000000, 64).unwrap().to_vec();
    assert_eq!(
        call(&mut host, &mut vm, 368, &[0, 7, 0x80000008]),
        NativeReturn::Value(0)
    );
    assert_eq!(vm.read(0x80000000, 64).unwrap(), before);
    assert_eq!(
        call(&mut host, &mut vm, 368, &[handle, 7, u32::MAX]),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 368, &[handle, 0, 0x80000008]),
        NativeReturn::Value(0)
    );
    assert_eq!(vm.read(0x80000008, 36).unwrap(), [0; 36]);
    assert_eq!(vm.read(0x80000000, 8).unwrap(), [0x5a; 8]);
    assert_eq!(vm.read(0x8000002c, 20).unwrap(), [0x5a; 20]);
}
