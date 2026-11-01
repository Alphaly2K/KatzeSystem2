use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, rc::Rc};

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
            b"load" => {
                for (native, address, result) in [(365, 32, 0), (590, 48, 4)] {
                    push(&mut code, address);
                    ins(&mut code, 0x2b, &[native, 4]);
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
