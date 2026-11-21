use katze_system2::{
    Error, Result,
    expression::State,
    host::{
        resources::{ResourceMode, ResourceServices},
        storage::{ReadSource, SharedSource},
    },
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Requests = Rc<RefCell<Vec<Vec<u8>>>>;

struct Files {
    value: u32,
    requests: Requests,
}

impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Ok([b"image/".as_slice(), name, b".hg3"].concat())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.requests.borrow_mut().push(name.to_vec());
        Ok(Some(match name {
            b"image/art.hg3" => {
                let mut data = b"HG-3".to_vec();
                data.extend([12u32, 0x300, 0, 7].into_iter().flat_map(u32::to_le_bytes));
                data.extend(b"stdinfo\0");
                data.extend(
                    [0u32, 40, 3, 2, 32, (-2i32) as u32, 5, 9, 8, 0, 6, 7]
                        .into_iter()
                        .flat_map(u32::to_le_bytes),
                );
                data
            }
            b"settings" => format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><value>{}</value><text>raw%d[G0]</text></document>",
                self.value
            )
            .into_bytes(),
            _ => return Err(Error::new(0, "合成资源名称错误")),
        }))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for n in args {
        code.push(3);
        code.extend(n.to_le_bytes());
    }
}

fn push(code: &mut Vec<u8>, value: u32) {
    ins(code, 0x25, &[value]);
}

fn load(code: &mut Vec<u8>, address: u32) {
    code.extend([0x25, 0x13]);
    code.extend(address.to_le_bytes());
}

struct Programs;

impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut data = vec![0; 256];
        data[32..36].copy_from_slice(b"art\0");
        data[48..57].copy_from_slice(b"settings\0");
        data[64..70].copy_from_slice(b"value\0");
        data[80..85].copy_from_slice(b"text\0");
        match name {
            b"gallery" | b"gallery_wrong_type" | b"gallery_released" => {
                data[96..99].copy_from_slice(b"bg\0");
                data[100..103].copy_from_slice(b"cg\0");
                data[104..112].copy_from_slice(b"missing\0");
                data[112..118].copy_from_slice(b"short\0");
                data[160..192].fill(0x5a);
                for (native, address, result) in [
                    (405, 96, 0),
                    (412, 100, 4),
                    (405, 104, 8),
                    (405, 112, 12),
                    (412, 104, 16),
                ] {
                    push(&mut code, address);
                    ins(&mut code, 0x2b, &[native, 4]);
                    ins(&mut code, 0x26, &[result]);
                }
                match name {
                    b"gallery" => {
                        for (native, handle, args, result) in [
                            (407, 0, &[][..], 20),
                            (407, 8, &[], 24),
                            (407, 12, &[], 28),
                            (408, 0, &[0], 32),
                            (408, 0, &[6], 36),
                            (409, 0, &[0, 3], 40),
                            (410, 0, &[0, 3], 44),
                            (411, 0, &[0, 0, 0, 0, 160], 48),
                            (411, 0, &[0, 3, 0, 0, 176], 52),
                            (414, 4, &[], 56),
                            (415, 4, &[0, 1], 60),
                            (416, 4, &[1, 3, 0x8000_0000], 64),
                            (416, 4, &[0, 1, 0x8000_0008], 68),
                            (416, 16, &[0, 0, u32::MAX], 72),
                        ] {
                            load(&mut code, handle);
                            for &value in args {
                                push(&mut code, value);
                            }
                            ins(&mut code, 0x2b, &[native, (args.len() as u32 + 1) * 4]);
                            ins(&mut code, 0x26, &[result]);
                        }
                        push(&mut code, 0x1234_5678);
                        for (native, handle) in [(406, 0), (406, 8), (406, 12), (413, 4), (413, 16)]
                        {
                            load(&mut code, handle);
                            ins(&mut code, 0x2b, &[native, 4]);
                        }
                        ins(&mut code, 0x26, &[76]);
                    }
                    b"gallery_wrong_type" => {
                        load(&mut code, 4);
                        ins(&mut code, 0x2b, &[406, 4]);
                    }
                    _ => {
                        load(&mut code, 4);
                        ins(&mut code, 0x2b, &[413, 4]);
                        load(&mut code, 4);
                        ins(&mut code, 0x2b, &[414, 4]);
                    }
                }
            }
            b"scene_exists" => {
                data[..4].copy_from_slice(&17u32.to_le_bytes());
                data[16..27].copy_from_slice(b"\x82\xa0%d[G0]\0\0\0");
                data[32..40].copy_from_slice(b"missing\0");
                for address in [16, 32, 96] {
                    push(&mut code, address);
                    ins(&mut code, 0x2b, &[322, 4]);
                }
            }
            b"mixed" => {
                for (native, args, result) in [
                    (112, vec![2, 1], 8),
                    (365, vec![32], 0),
                    (419, vec![], 12),
                    (590, vec![48], 4),
                    (515, vec![], 16),
                ] {
                    for value in &args {
                        push(&mut code, *value);
                    }
                    ins(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
                    ins(&mut code, 0x26, &[result]);
                }
            }
            b"metadata" => {
                data[48..60].copy_from_slice(b"art%d[G16]\0\0");
                data[16..20].copy_from_slice(&7u32.to_le_bytes());
                data[124..240].fill(0x5a);
                for (name, output, result) in
                    [(48, 128, 0), (64, 164, 4), (80, 200, 8), (48, u32::MAX, 12)]
                {
                    push(&mut code, name);
                    push(&mut code, output);
                    ins(&mut code, 0x2b, &[375, 8]);
                    ins(&mut code, 0x26, &[result]);
                }
            }
            b"load" => {
                for (native, address, result) in [(365, 32, 0), (590, 48, 4)] {
                    push(&mut code, address);
                    ins(&mut code, 0x2b, &[native, 4]);
                    ins(&mut code, 0x26, &[result]);
                }
            }
            b"save_raw" => {
                for (native, args, result) in [(365, vec![32], 0), (238, vec![7], 4)] {
                    for value in &args {
                        push(&mut code, *value);
                    }
                    ins(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
                    ins(&mut code, 0x26, &[result]);
                }
                load(&mut code, 4);
                push(&mut code, 99);
                load(&mut code, 0);
                ins(&mut code, 0x2b, &[387, 12]);
                ins(&mut code, 0x26, &[8]);
                for (result, args) in [(12, [0, 99, u32::MAX]), (16, [u32::MAX, 99, 0])] {
                    for value in args {
                        push(&mut code, value);
                    }
                    ins(&mut code, 0x2b, &[387, 12]);
                    ins(&mut code, 0x26, &[result]);
                }
            }
            b"consume" => {
                load(&mut code, 4);
                for n in [99, 64] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[592, 12]);
                ins(&mut code, 0x26, &[8]);
                push(&mut code, 0xabcd_ef01);
                load(&mut code, 4);
                for n in [96, 80] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[593, 12]);
                ins(&mut code, 0x26, &[12]);
                load(&mut code, 0);
                for n in [7, 128] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[368, 12]);
                ins(&mut code, 0x26, &[16]);
                load(&mut code, 0);
                push(&mut code, 196);
                ins(&mut code, 0x2b, &[431, 8]);
                ins(&mut code, 0x26, &[120]);
                push(&mut code, 0x1234_5678);
                for (address, native) in [(0, 366), (4, 591)] {
                    load(&mut code, address);
                    ins(&mut code, 0x2b, &[native, 4]);
                }
                ins(&mut code, 0x26, &[20]);
                push(&mut code, 48);
                ins(&mut code, 0x2b, &[590, 4]);
                ins(&mut code, 0x26, &[24]);
                load(&mut code, 24);
                for n in [99, 64] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[592, 12]);
                ins(&mut code, 0x26, &[28]);
            }
            b"null" => {
                for n in [0, 99, u32::MAX] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[592, 12]);
                ins(&mut code, 0x26, &[0]);
                push(&mut code, 0xabcd_ef01);
                for (native, args) in [
                    (593, vec![0, u32::MAX, u32::MAX]),
                    (366, vec![0]),
                    (591, vec![0]),
                ] {
                    for n in &args {
                        push(&mut code, *n);
                    }
                    ins(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
                }
                ins(&mut code, 0x26, &[4]);
                for n in [0, 7, u32::MAX] {
                    push(&mut code, n);
                }
                ins(&mut code, 0x2b, &[368, 12]);
                ins(&mut code, 0x26, &[8]);
                push(&mut code, 48);
                ins(&mut code, 0x2b, &[590, 4]);
                push(&mut code, 0xfeed_face);
                ins(&mut code, 0x26, &[12]);
            }
            name if name.len() == 4 => {
                let native = u32::from_le_bytes(name.try_into().unwrap());
                assert!(matches!(native, 114 | 366 | 420 | 516 | 591));
                load(&mut code, 0);
                ins(&mut code, 0x2b, &[native, 4]);
            }
            _ => unreachable!(),
        }
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

fn runtime() -> Runtime<KcsFactory<Programs>> {
    Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Programs),
    )
}

#[test]
fn gallery_tables_keep_sparse_slots_and_typed_handles() -> Result<()> {
    struct GalleryFiles;

    impl ResourceServices for GalleryFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"fes");
            Ok([b"fes/".as_slice(), name].concat())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            let body = match name {
                b"fes/bg" => {
                    let mut data = vec![0; 0x15180];
                    for at in [0, 3 * 0x21c, 6 * 8 * 0x21c] {
                        data[at] = 1;
                        data[at + 4..at + 8].copy_from_slice(b"sky\0");
                        data[at + 0x14..at + 0x18].copy_from_slice(&1u32.to_le_bytes());
                        data[at + 0x118..at + 0x11c].copy_from_slice(&1u32.to_le_bytes());
                    }
                    data[0x18..0x1c].copy_from_slice(b"A0CB");
                    data[0x11c..0x120].copy_from_slice(b"0\0D0");
                    let at = 3 * 0x21c + 0x118;
                    data[at..at + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
                    data
                }
                b"fes/cg" => {
                    let mut data = vec![0; 4 + 2 * 128];
                    data[0] = 2;
                    data[228..232].copy_from_slice(b"art\0");
                    data
                }
                b"fes/short" => vec![0; 12],
                b"fes/missing" => return Ok(None),
                _ => return Err(Error::new(0, "合成资源名称错误")),
            };
            Ok(Some(body))
        }
    }

    let mut game = runtime();
    game.factory_mut().external = katze_system2::memory::SharedMemory::new(vec![0x5a; 16])?;
    game.set_resources(GalleryFiles);
    let task = game.create(0, kind::ORDINARY, b"gallery", b"")?;
    game.update(0, 1)?;
    let vm = game.object(task).unwrap().interpreter().unwrap();
    let handles = vm.read(0, 20)?;
    assert!(handles[..16].chunks_exact(4).all(|handle| handle != [0; 4]));
    assert_eq!(&handles[16..], &[0; 4]);
    let expected: Vec<_> = [
        2u32,
        0,
        0,
        2,
        1,
        1,
        0x1234_5678,
        1,
        0,
        2,
        0,
        1,
        0,
        0,
        0x1234_5678,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    assert_eq!(vm.read(20, expected.len())?, expected);
    assert_eq!(vm.read(160, 14)?, b"sky,A,0,D,B\0\0Z".as_slice());
    assert_eq!(vm.read(176, 16)?, [0x5a; 16]);
    assert_eq!(vm.read(0x8000_0000, 16)?, b"art\0ZZZZZZZZZZZZ".as_slice());

    for program in [b"gallery_wrong_type".as_slice(), b"gallery_released"] {
        let mut runtime = runtime();
        runtime.set_resources(GalleryFiles);
        runtime.create(0, kind::ORDINARY, program, b"")?;
        let error = runtime.update(0, 1).unwrap_err();
        assert!(error.to_string().contains("图鉴表句柄无效"));
    }
    Ok(())
}

#[test]
fn scene_probe_opens_and_releases_wide_resources_without_reading() -> Result<()> {
    struct EmptySource;

    impl ReadSource for EmptySource {
        fn len(&self) -> Result<u64> {
            Ok(0)
        }

        fn read_at(&self, _: u64, _: &mut [u8]) -> Result<usize> {
            panic!("the scene probe must not read the source")
        }
    }

    #[derive(Default)]
    struct SceneFiles {
        paths: Rc<RefCell<Vec<String>>>,
        opened: Rc<RefCell<Vec<std::sync::Weak<dyn ReadSource>>>>,
    }

    impl ResourceServices for SceneFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"scene");
            Ok([b"scene/".as_slice(), name, b".cst\0ignored"].concat())
        }

        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("the scene probe must open without reading")
        }

        fn open_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<SharedSource>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            let name = String::from_utf16(name).unwrap();
            self.paths.borrow_mut().push(name.clone());
            if name == "scene/\u{3042}17.cst" {
                let source: SharedSource = Arc::new(EmptySource);
                self.opened.borrow_mut().push(Arc::downgrade(&source));
                return Ok(Some(source));
            }
            Ok(None)
        }
    }

    let services = SceneFiles::default();
    let paths = services.paths.clone();
    let opened = services.opened.clone();
    let mut runtime = runtime();
    runtime.set_resources(katze_system2::host::resources::CachedResources::new(
        services,
    ));
    let task = runtime.create(0, kind::ORDINARY, b"scene_exists", b"")?;
    runtime.update(0, 1)?;
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(
        vm.stack_bytes(),
        [1u32, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        *paths.borrow(),
        ["scene/\u{3042}17.cst", "scene/missing.cst", "scene/.cst"]
    );
    assert!(opened.borrow()[0].upgrade().is_none());
    Ok(())
}

#[test]
fn named_image_metadata_reads_only_the_prefix_and_preserves_unwritten_outputs() {
    struct Prefix {
        data: Vec<u8>,
        short: bool,
    }

    impl ReadSource for Prefix {
        fn len(&self) -> Result<u64> {
            Ok(80_000)
        }
        fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize> {
            assert!(offset + output.len() as u64 <= 1024);
            if self.short && offset > 0 {
                return Ok(0);
            }
            if offset == 0 {
                assert_eq!(output.len(), 1024);
                output[..self.data.len()].copy_from_slice(&self.data);
            }
            Ok(if self.short {
                self.data.len()
            } else {
                output.len()
            })
        }
    }

    struct MetadataFiles;

    impl ResourceServices for MetadataFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            Ok([b"image/".as_slice(), name, b".hg3"].concat())
        }
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("named metadata must not load the whole image")
        }
        fn open(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<SharedSource>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            let short = match name {
                b"image/art7.hg3" => false,
                b"image/value.hg3" => true,
                b"image/text.hg3" => return Ok(None),
                _ => panic!("unexpected resolved image name"),
            };
            let mut data = b"HG-3".to_vec();
            data.extend(
                [12u32, 0x300, 2048, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            data.extend(b"stdinfo\0");
            data.extend(
                [56u32, 40, 3, 2, 32, (-2i32) as u32, 5, 9, 8, 0, 6, 7]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            data.extend(b"img0000\0");
            data.extend([0u32, 1920].into_iter().flat_map(u32::to_le_bytes));
            Ok(Some(Arc::new(Prefix { data, short })))
        }
    }

    let mut runtime = runtime();
    runtime.set_resources(MetadataFiles);
    let task = runtime.create(0, kind::ORDINARY, b"metadata", b"").unwrap();
    runtime.update(0, 0).unwrap();
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(
        vm.read(0, 16).unwrap(),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let fields: Vec<_> = [-2i32, 5, 3, 2, 9, 8, 4, 6, 7]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    assert_eq!(vm.read(128, 36).unwrap(), fields);
    assert_eq!(vm.read(124, 4).unwrap(), [0x5a; 4]);
    assert_eq!(vm.read(164, 76).unwrap(), [0x5a; 76]);
}

#[test]
fn raw_image_records_keep_source_bytes_after_the_loading_task_is_removed() {
    for budget in [1, 4096] {
        let mut runtime = runtime();
        runtime.set_budget(budget, 1 << 20);
        runtime.set_resources(Files {
            value: 17,
            requests: Requests::default(),
        });
        let task = runtime.create(0, kind::ORDINARY, b"save_raw", b"").unwrap();
        runtime.update(1, 1).unwrap();
        let bytes = runtime
            .object(task)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(0, 20)
            .unwrap()
            .to_vec();
        let values: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(&values[2..], &[1, 0, 0]);
        let original = runtime.image_data(values[0]).unwrap().to_vec();
        runtime.destroy(task).unwrap();
        runtime.update(2, 1).unwrap();
        assert_eq!(
            runtime
                .host()
                .data_container(values[1])
                .unwrap()
                .record(99)
                .unwrap(),
            Some(original.as_slice())
        );
    }
}

#[test]
fn runtime_resource_handles_survive_task_release_and_service_replacement_until_explicit_close() {
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut runtime = runtime();
        runtime.set_budget(budget, 1 << 20);
        runtime.set_resources(Files {
            value: 17,
            requests: requests.clone(),
        });
        let producer = runtime.create(0, kind::ORDINARY, b"load", b"").unwrap();
        runtime.update(10, 1).unwrap();
        let vm = runtime.object(producer).unwrap().interpreter().unwrap();
        let handles = vm.read(0, 8).unwrap().to_vec();
        let raw = u32::from_le_bytes(handles[..4].try_into().unwrap());
        let xml = u32::from_le_bytes(handles[4..].try_into().unwrap());
        assert_ne!(raw, 0);
        assert_ne!(xml, 0);
        assert_ne!(raw, xml);
        assert!(runtime.image_data(raw).unwrap().starts_with(b"HG-3"));
        assert!(runtime.images().get(raw).is_err());
        runtime.destroy(producer).unwrap();
        runtime.update(15, 1).unwrap();
        assert!(runtime.image_data(raw).is_ok());
        runtime.set_resources(Files {
            value: 23,
            requests: requests.clone(),
        });
        let consumer = runtime.create(0, kind::ORDINARY, b"consume", b"").unwrap();
        assert_eq!(consumer, producer);
        runtime
            .object_mut(consumer)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(0, &handles)
            .unwrap();
        runtime.update(20, 1).unwrap();
        let vm = runtime.object(consumer).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(8, 16).unwrap(),
            [17u32, 0xabcd_ef01, 1, 0x1234_5678]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(vm.read_string(96).unwrap(), b"raw%d[G0]");
        assert_eq!(vm.read(120, 4).unwrap(), 1u32.to_le_bytes());
        assert_eq!(
            vm.read(196, 36).unwrap(),
            [-2i32, 5, 3, 2, 9, 8, 0, 6, 7]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            vm.read(128, 36).unwrap(),
            [-2i32, 5, 3, 2, 9, 8, 4, 6, 7]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let next = u32::from_le_bytes(vm.read(24, 4).unwrap().try_into().unwrap());
        assert!(next > raw && next > xml);
        assert_eq!(vm.read(28, 4).unwrap(), 23u32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
        assert!(runtime.image_data(raw).is_err());
        assert_eq!(
            *requests.borrow(),
            [
                b"image/art.hg3".to_vec(),
                b"settings".to_vec(),
                b"settings".to_vec()
            ]
        );
    }
}

#[test]
fn null_resource_operations_work_without_services_and_actual_load_reports_missing_service() {
    let mut runtime = runtime();
    runtime.set_budget(1, 1 << 20);
    let id = runtime.create(0, kind::ORDINARY, b"null", b"").unwrap();
    let error = runtime.update(0, 0).unwrap_err();
    assert_eq!(error.message, "运行时未配置资源服务");
    let vm = runtime.object(id).unwrap().interpreter().unwrap();
    assert_eq!(
        vm.read(0, 16).unwrap(),
        [99u32, 0xabcd_ef01, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn mixed_runtime_handles_reject_other_types_and_never_reuse_released_numbers() {
    fn handles(runtime: &Runtime<KcsFactory<Programs>>, id: u32) -> Vec<u32> {
        runtime
            .object(id)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(0, 20)
            .unwrap()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect()
    }

    for budget in [1, 4096] {
        let mut runtime = runtime();
        runtime.set_budget(budget, 1 << 20);
        runtime.set_resources(Files {
            value: 17,
            requests: Requests::default(),
        });
        let producer = runtime.create(0, kind::ORDINARY, b"mixed", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let original = handles(&runtime, producer);
        let unique: std::collections::BTreeSet<_> = original.iter().copied().collect();
        assert_eq!(unique.len(), 5);
        assert!(!unique.contains(&0));
        for (kind, handle) in original.iter().enumerate() {
            assert_eq!(runtime.images().get(*handle).is_ok(), kind == 2);
            assert_eq!(runtime.image_data(*handle).is_ok(), kind == 0);
        }

        for (correct_type, native) in [366u32, 591, 114, 420, 516].into_iter().enumerate() {
            for (other_type, handle) in original.iter().enumerate() {
                if other_type == correct_type {
                    continue;
                }
                let id = runtime
                    .create(0, kind::ORDINARY, &native.to_le_bytes(), b"")
                    .unwrap();
                runtime
                    .object_mut(id)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(0, &handle.to_le_bytes())
                    .unwrap();
                let error = runtime.update(0, 0).unwrap_err();
                assert!(error.message.contains("句柄"), "{error}");
                runtime.update(0, 0).unwrap();
                runtime.destroy(id).unwrap();
            }
        }
        // 每个原对象仍能由正确的入口释放，交叉释放没有损坏其存储。
        for (native, handle) in [366u32, 591, 114, 420, 516].into_iter().zip(&original) {
            let id = runtime
                .create(0, kind::ORDINARY, &native.to_le_bytes(), b"")
                .unwrap();
            runtime
                .object_mut(id)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(0, &handle.to_le_bytes())
                .unwrap();
            runtime.update(0, 0).unwrap();
            runtime.destroy(id).unwrap();
        }
        assert!(runtime.images().get(original[2]).is_err());
        assert!(runtime.image_data(original[0]).is_err());
        runtime.destroy(producer).unwrap();
        runtime.update(0, 0).unwrap();

        let next = runtime.create(0, kind::ORDINARY, b"mixed", b"").unwrap();
        assert_eq!(next, producer);
        runtime.update(0, 0).unwrap();
        let second = handles(&runtime, next);
        assert!(second.iter().all(|handle| handle > unique.last().unwrap()));
        runtime.destroy(next).unwrap();
        assert!(runtime.images().get(second[2]).is_err());
        assert!(runtime.image_data(second[0]).is_ok());

        runtime.set_resources(Files {
            value: 23,
            requests: Requests::default(),
        });
        let next = runtime.create(0, kind::ORDINARY, b"mixed", b"").unwrap();
        runtime.update(0, 0).unwrap();
        let third = handles(&runtime, next);
        assert!(
            third
                .iter()
                .all(|handle| handle > second.iter().max().unwrap())
        );
        assert!(runtime.image_data(second[0]).is_ok());
        assert!(runtime.images().get(second[2]).is_err());
    }
}
