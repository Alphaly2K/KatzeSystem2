use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    save::SaveServices,
};
use std::{cell::RefCell, io::Write, rc::Rc};

type Requests = Rc<RefCell<Vec<(u32, Vec<u8>)>>>;

struct Saves(Requests);
impl SaveServices for Saves {
    fn read(&mut self, task: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
        self.0.borrow_mut().push((task, name.to_vec()));
        assert_eq!(name, b"slot-7.css");
        let mut compressed =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        compressed
            .write_all(
                &[7u32, 6]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        compressed.write_all(b"abcdEF").unwrap();
        compressed
            .write_all(
                &[7u32, 4]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        compressed.write_all(b"rest").unwrap();
        let bytes = compressed.finish().unwrap();
        let mut file = b"CSS\0".to_vec();
        file.extend(
            [bytes.len() as u32, 26, 0xfedc_ba98]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        file.extend(bytes);
        Ok(Some(file))
    }
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for n in args {
        code.push(3);
        code.extend(n.to_le_bytes());
    }
}
fn push(code: &mut Vec<u8>, n: u32) {
    ins(code, 0x25, &[n]);
}
fn load(code: &mut Vec<u8>, address: u32) {
    code.extend([0x25, 0x13]);
    code.extend(address.to_le_bytes());
}
fn native(code: &mut Vec<u8>, n: u32, count: u32) {
    ins(code, 0x2b, &[n, count * 4]);
}
fn store(code: &mut Vec<u8>, address: u32) {
    ins(code, 0x26, &[address]);
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut data = vec![0; 128];
        data[48..52].copy_from_slice(&7u32.to_le_bytes());
        data[64..81].copy_from_slice(b"slot-%d[G48].css\0");
        if name == b"load" {
            push(&mut code, 64);
            native(&mut code, 247, 1);
            store(&mut code, 0);
            load(&mut code, 0);
            native(&mut code, 248, 1);
            store(&mut code, 4);
            native(&mut code, 419, 0);
            store(&mut code, 8);
            load(&mut code, 0);
            for n in [7, 88, 4] {
                push(&mut code, n);
            }
            native(&mut code, 254, 4);
            store(&mut code, 12);
            push(&mut code, 0xfeed_face);
            load(&mut code, 0);
            push(&mut code, 7);
            native(&mut code, 469, 2);
            store(&mut code, 16);
            load(&mut code, 0);
            for n in [7, 96, 8] {
                push(&mut code, n);
            }
            native(&mut code, 254, 4);
            store(&mut code, 20);
        } else {
            assert_eq!(name, b"consume");
            load(&mut code, 0);
            native(&mut code, 389, 1);
            store(&mut code, 24);
            load(&mut code, 0);
            native(&mut code, 248, 1);
            store(&mut code, 28);
            load(&mut code, 0);
            native(&mut code, 246, 1);
            push(&mut code, 64);
            native(&mut code, 238, 1);
            store(&mut code, 32);
        }
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

#[test]
fn runtime_loads_named_css_and_keeps_containers_until_explicit_release_across_tasks() {
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Programs),
        );
        rt.set_budget(budget, 1 << 20);
        rt.set_save_services(Saves(requests.clone()));
        let producer = rt.create(0, kind::ORDINARY, b"load", b"").unwrap();
        rt.update(0, 0).unwrap();
        let vm = rt.object(producer).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(0, 24)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let handle = values[0];
        assert_ne!(handle, 0);
        assert_ne!(handle, values[2]);
        assert_eq!(
            (values[1], values[3], values[4], values[5]),
            (0xfedc_ba98, 4, 0xfeed_face, 4)
        );
        assert_eq!(vm.read(88, 4).unwrap(), b"abcd");
        assert_eq!(vm.read(96, 4).unwrap(), b"rest");
        assert!(rt.host().data_container(values[2]).is_err());
        assert!(rt.images().get(handle).is_err());
        rt.destroy(producer).unwrap();
        rt.update(1, 0).unwrap();
        assert_eq!(
            rt.host().data_container(handle).unwrap().record(7).unwrap(),
            Some(b"rest".as_slice())
        );
        rt.set_save_services(Saves(requests.clone()));
        let consumer = rt.create(0, kind::ORDINARY, b"consume", b"").unwrap();
        rt.object_mut(consumer)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(0, &handle.to_le_bytes())
            .unwrap();
        rt.update(2, 0).unwrap();
        let vm = rt.object(consumer).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(24, 12)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(&values[..2], &[1, 0xfedc_ba98]);
        assert!(values[2] > handle);
        assert!(rt.host().data_container(handle).is_err());
        assert_eq!(rt.host().data_container(values[2]).unwrap().tag(), 64);
        assert_eq!(*requests.borrow(), [(producer, b"slot-7.css".to_vec())]);
    }
}

#[test]
fn data_natives_preserve_null_paths_and_validate_vm_spans_handles_and_services() {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 32], 32).unwrap(), vec![]).unwrap();
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
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
            vm,
        )
    }
    for index in [244, 245, 248, 254, 389] {
        assert_eq!(
            call(&mut host, &mut vm, index, &[0]).unwrap(),
            NativeReturn::Value(0)
        );
    }
    for index in [246, 469] {
        assert_eq!(
            call(&mut host, &mut vm, index, &[0]).unwrap(),
            NativeReturn::Continue
        );
    }
    let NativeReturn::Value(handle) = call(&mut host, &mut vm, 238, &[99]).unwrap() else {
        panic!()
    };
    vm.write(0, b"zero").unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 244, &[handle, 7, 4, 0]).unwrap(),
        NativeReturn::Value(1)
    );
    assert_eq!(
        call(&mut host, &mut vm, 244, &[handle, 8, 0, u32::MAX]).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 254, &[handle, 7, u32::MAX, 4]).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 254, &[handle, 8, u32::MAX, 4]).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 254, &[handle, 7, 8, 3]).unwrap(),
        NativeReturn::Value(3)
    );
    assert_eq!(vm.read(8, 4).unwrap(), b"zer\0");
    assert!(call(&mut host, &mut vm, 254, &[handle, 7, 63, 4]).is_err());
    assert!(call(&mut host, &mut vm, 244, &[handle, 7, 4, 63]).is_err());
    assert_eq!(
        call(&mut host, &mut vm, 245, &[handle, 0])
            .unwrap_err()
            .message,
        "宿主未配置存档写入服务"
    );
    host.save_services = Some(Box::new(Saves(Requests::default())));
    assert_eq!(
        call(&mut host, &mut vm, 245, &[handle, 0])
            .unwrap_err()
            .message,
        "存档写入服务尚未接入"
    );
    host.save_services = None;
    call(&mut host, &mut vm, 246, &[handle]).unwrap();
    assert!(call(&mut host, &mut vm, 248, &[handle]).is_err());
    vm.write(0, b"slot.css\0").unwrap();
    assert_eq!(
        call(&mut host, &mut vm, 247, &[0]).unwrap_err().message,
        "宿主未配置存档读取服务"
    );
}

#[test]
fn container_writes_report_failure_then_reload_the_saved_records_without_consuming_the_source() {
    #[derive(Default)]
    struct Files {
        writes: Vec<(u32, Vec<u8>, Vec<u8>)>,
        saved: Option<Vec<u8>>,
    }
    struct MemoryFiles(Rc<RefCell<Files>>);
    impl SaveServices for MemoryFiles {
        fn read(&mut self, _: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
            assert_eq!(name, b"slot-7.css");
            Ok(self.0.borrow().saved.clone())
        }
        fn write(&mut self, task: u32, name: &[u8], bytes: &[u8]) -> Result<bool> {
            let mut files = self.0.borrow_mut();
            files.writes.push((task, name.to_vec(), bytes.to_vec()));
            if files.writes.len() == 1 {
                return Ok(false);
            }
            files.saved = Some(bytes.to_vec());
            Ok(true)
        }
    }
    struct WritePrograms;
    impl ProgramLoader for WritePrograms {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            push(&mut code, 0xdead_beef);
            push(&mut code, 0xfedc_ba98);
            native(&mut code, 238, 1);
            store(&mut code, 0);
            load(&mut code, 0);
            for value in [9, 5, 96] {
                push(&mut code, value);
            }
            native(&mut code, 244, 4);
            store(&mut code, 4);
            for output in [8, 12] {
                load(&mut code, 0);
                push(&mut code, 64);
                native(&mut code, 245, 2);
                store(&mut code, output);
            }
            push(&mut code, 64);
            native(&mut code, 247, 1);
            store(&mut code, 16);
            ins(&mut code, 0x3b, &[]);
            let mut data = vec![0; 128];
            data[48..52].copy_from_slice(&7u32.to_le_bytes());
            data[64..81].copy_from_slice(b"slot-%d[G48].css\0");
            data[96..101].copy_from_slice(b"hello");
            Ok(Some(Program::from_code(&code, data, 128)?))
        }
    }
    for budget in [1, 4096] {
        let files = Rc::new(RefCell::new(Files::default()));
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(WritePrograms),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.set_save_services(MemoryFiles(files.clone()));
        let task = runtime.create(0, kind::ORDINARY, b"write", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(0, 20)
            .unwrap()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(&values[1..4], &[1, 0, 1]);
        assert_ne!(values[0], values[4]);
        for handle in [values[0], values[4]] {
            let data = runtime.host().data_container(handle).unwrap();
            assert_eq!(data.tag(), 0xfedc_ba98);
            assert_eq!(data.record(9).unwrap(), Some(b"hello".as_slice()));
        }
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
        let files = files.borrow();
        assert_eq!(files.writes.len(), 2);
        assert_eq!(files.writes[0], files.writes[1]);
        assert_eq!(files.writes[0].0, task);
        assert_eq!(files.writes[0].1, b"slot-7.css");
    }
}

#[test]
fn nested_containers_keep_first_snapshot_capacity_and_independent_lifetimes() {
    fn call(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> u32 {
        let NativeReturn::Value(value) = host
            .call(
                index,
                &args
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
                vm,
            )
            .unwrap()
        else {
            panic!("expected value")
        };
        value
    }
    fn release(host: &mut impl Host, vm: &mut Interpreter, handle: u32) {
        assert_eq!(
            host.call(246, &handle.to_le_bytes(), vm).unwrap(),
            NativeReturn::Continue
        );
    }
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(
        Program::from_code(&[], b"abclater".to_vec(), 64).unwrap(),
        vec![],
    )
    .unwrap();
    let outer = call(&mut host, &mut vm, 238, &[9]);
    let inner = call(&mut host, &mut vm, 238, &[0xfedc_ba98]);
    for (length, address) in [(3, 0), (5, 3)] {
        assert_eq!(
            call(&mut host, &mut vm, 244, &[inner, 17, length, address]),
            1
        );
    }
    host.call(
        469,
        &[inner, 17]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
        &mut vm,
    )
    .unwrap();
    assert_eq!(call(&mut host, &mut vm, 242, &[outer, 44, inner]), 1);
    assert_eq!(call(&mut host, &mut vm, 389, &[inner]), 1);
    assert_eq!(call(&mut host, &mut vm, 242, &[outer, 44, inner]), 1);
    assert_eq!(call(&mut host, &mut vm, 242, &[0, 44, u32::MAX]), 0);
    assert_eq!(call(&mut host, &mut vm, 242, &[u32::MAX, 44, 0]), 0);
    assert_eq!(call(&mut host, &mut vm, 252, &[0]), 0);
    assert_eq!(call(&mut host, &mut vm, 252, &[outer, 45]), 0);
    let restored = call(&mut host, &mut vm, 252, &[outer, 44]);
    release(&mut host, &mut vm, inner);
    release(&mut host, &mut vm, outer);
    let data = host.data_container(restored).unwrap();
    assert_eq!(data.tag(), 0xfedc_ba98);
    assert_eq!((data.capacity(), data.used_len()), (0x80000 - 11, 13));
    assert_eq!(data.record(17).unwrap(), Some(b"later".as_slice()));
    assert_eq!(call(&mut host, &mut vm, 242, &[restored, 55, restored]), 1);
    let copy = call(&mut host, &mut vm, 252, &[restored, 55]);
    release(&mut host, &mut vm, restored);
    let data = host.data_container(copy).unwrap();
    assert_eq!(data.record(17).unwrap(), Some(b"later".as_slice()));
    assert_eq!(data.record(55).unwrap(), None);
    let mut bytes = data.snapshot().unwrap();
    assert_eq!(
        &bytes[..16],
        [0u32, 0x80000 - 11, 13, 0xfedc_ba98]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        katze_system2::save::DataContainer::from_snapshot(&bytes)
            .unwrap()
            .record(17)
            .unwrap(),
        Some(b"later".as_slice())
    );
    bytes.pop();
    assert!(katze_system2::save::DataContainer::from_snapshot(&bytes).is_err());
    bytes[4..8].copy_from_slice(&12u32.to_le_bytes());
    assert!(katze_system2::save::DataContainer::from_snapshot(&bytes).is_err());
    bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(katze_system2::save::DataContainer::from_snapshot(&bytes).is_err());
}

struct RestorePrograms;
impl ProgramLoader for RestorePrograms {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut code = Vec::new();
        let mut data = vec![0; 128];
        data[64..71].copy_from_slice(b"1234567");
        if name == b"restore" {
            push(&mut code, 99);
            native(&mut code, 238, 1);
            store(&mut code, 0);
            for (i, args) in [
                [11, 1, 1, 9],
                [11, 2, 0, 1],
                [12, 2, 4, 1],
                [13, 2, 0, 0],
                [14, 999, 0, 1],
            ]
            .into_iter()
            .enumerate()
            {
                load(&mut code, 0);
                for value in args {
                    push(&mut code, value);
                }
                native(&mut code, 240, 5);
                store(&mut code, 80 + i as u32 * 4);
            }
            for (id, length) in [(9, 7), (10, 0)] {
                load(&mut code, 0);
                for value in [id, length, 64] {
                    push(&mut code, value);
                }
                native(&mut code, 244, 4);
                ins(&mut code, 0x2d, &[]);
            }
            for (index, (record, target, start)) in [
                (9, 1, 2),
                (9, 2, 0),
                (9, 1, 4),
                (10, 1, 1),
                (8, 1, 0),
                (9, 0, 0),
                (9, 999, 0),
            ]
            .into_iter()
            .enumerate()
            {
                load(&mut code, 0);
                for value in [record, target, start] {
                    push(&mut code, value);
                }
                native(&mut code, 250, 4);
                store(&mut code, 4 + index as u32 * 4);
            }
            push(&mut code, 1);
            native(&mut code, 67, 1);
            load(&mut code, 0);
            for value in [9, 1, 1] {
                push(&mut code, value);
            }
            native(&mut code, 250, 4);
            store(&mut code, 32);
            push(&mut code, 0);
            native(&mut code, 250, 1);
            store(&mut code, 36);
            for value in [0xdead_beef, 9, 999, 0] {
                push(&mut code, value);
            }
            native(&mut code, 250, 4);
            store(&mut code, 40);
            load(&mut code, 0);
            for value in [14, 1, 0, 1] {
                push(&mut code, value);
            }
            native(&mut code, 240, 5);
            store(&mut code, 100);
            push(&mut code, 0);
            native(&mut code, 240, 1);
            store(&mut code, 104);
            for value in [0xdead_beef, 14, 999, 0, 1] {
                push(&mut code, value);
            }
            native(&mut code, 240, 5);
            store(&mut code, 108);
            load(&mut code, 0);
            for value in [11, 2, 1] {
                push(&mut code, value);
            }
            native(&mut code, 250, 4);
            store(&mut code, 112);
        } else {
            assert_eq!(name, b"idle");
        }
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, data, 256)?))
    }
}

#[test]
fn shared_records_query_idle_and_running_tasks_and_copy_independent_clipped_values() {
    for budget in [1, 4096] {
        let mut numbers = State::new(4);
        numbers.shared.copy_from_slice(&[11, 22, 33, 44]);
        numbers.local[0] = 55;
        numbers.strings.insert(7, b"keep".to_vec());
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            numbers,
            KcsFactory::new(RestorePrograms),
        );
        rt.set_budget(budget, 1 << 20);
        assert_eq!(rt.create(0, kind::ORDINARY, b"idle", b"").unwrap(), 1);
        let task = rt.create(0, kind::ORDINARY, b"restore", b"").unwrap();
        assert_eq!(task, 2);
        rt.update(0, 0).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        let results: Vec<_> = vm
            .read(4, 40)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(results, [1, 1, 1, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            vm.read(80, 36).unwrap(),
            [1u32, 1, 1, 1, 0, 0, 0, 0, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let handle = u32::from_le_bytes(vm.read(0, 4).unwrap().try_into().unwrap());
        let records = rt.host().data_container(handle).unwrap();
        assert_eq!(
            records.record(11).unwrap().unwrap(),
            [22i32, 33, 44]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(records.record(12).unwrap(), Some(b"".as_slice()));
        assert_eq!(records.record(13).unwrap(), Some(b"".as_slice()));
        assert_eq!(records.record(14).unwrap(), None);
        assert_eq!(rt.numbers().shared, [0x3433_3231, 22, 33, 44]);
        assert_eq!(rt.numbers().local[0], 55);
        assert_eq!(rt.numbers().strings[&7], b"keep");
    }
}

#[test]
fn raw_buffer_container_roundtrip_survives_producer_exit_and_frees_by_base_address() {
    struct RawPrograms;
    impl ProgramLoader for RawPrograms {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            let mut code = vec![];
            push(&mut code, 0xdeadbeef);
            push(&mut code, 1);
            native(&mut code, 238, 1);
            store(&mut code, 0);
            load(&mut code, 0);
            push(&mut code, 7);
            if name == b"producer" {
                push(&mut code, 4);
                push(&mut code, 64);
                native(&mut code, 244, 4);
                store(&mut code, 8);
            } else {
                push(&mut code, 3);
                load(&mut code, 0x80000008);
                native(&mut code, 835, 4);
                store(&mut code, 8);
                load(&mut code, 0x80000000);
                native(&mut code, 834, 1);
            }
            load(&mut code, 0);
            push(&mut code, 7);
            push(&mut code, 0x80000004);
            native(&mut code, 836, 3);
            store(
                &mut code,
                if name == b"producer" {
                    0x80000000
                } else {
                    0x80000010
                },
            );
            load(&mut code, 0);
            native(&mut code, 246, 1);
            native(&mut code, 65, 0);
            if name != b"producer" {
                load(&mut code, 0x80000010);
                native(&mut code, 290, 1);
                push(&mut code, 0);
                native(&mut code, 833, 1);
                store(&mut code, 12);
                load(&mut code, 12);
                native(&mut code, 834, 1);
                push(&mut code, 0);
                native(&mut code, 834, 1);
            }
            code.push(0x3b);
            let mut data = vec![0; 128];
            data[64..68].copy_from_slice(b"ABCD");
            Ok(Some(Program::from_code(&code, data, 64)?))
        }
    }
    for budget in [1, 4096] {
        let external = katze_system2::memory::SharedMemory::new(vec![0; 32]).unwrap();
        let mut factory = KcsFactory::new(RawPrograms);
        factory.external = external.clone();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            factory,
        );
        runtime.set_budget(budget, 1 << 20);
        let producer = runtime.create(0, kind::ORDINARY, b"producer", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let raw = u32::from_le_bytes(external.read(0, 4).unwrap()[..].try_into().unwrap());
        assert_eq!(runtime.host().raw_buffer(raw, 4).unwrap(), b"ABCD");
        assert!(raw.is_multiple_of(4));
        assert!(runtime.host().raw_buffer(raw + 1, 4).is_err());
        assert!(runtime.host_mut().raw_buffer_mut(raw + 4, 1).is_err());
        runtime.destroy(producer).unwrap();
        runtime.host_mut().raw_buffer_mut(raw + 1, 1).unwrap()[0] = b'Z';
        external.write(8, &(raw + 1).to_le_bytes()).unwrap();
        let consumer = runtime.create(0, kind::ORDINARY, b"consumer", b"").unwrap();
        runtime.update(1, 1).unwrap();
        let copied = u32::from_le_bytes(external.read(16, 4).unwrap()[..].try_into().unwrap());
        assert_eq!(external.read(4, 4).unwrap().as_ref(), 3u32.to_le_bytes());
        assert_eq!(runtime.host().raw_buffer(copied, 3).unwrap(), b"ZCD");
        assert!(runtime.host().raw_buffer(raw, 1).is_err());
        assert_eq!(
            runtime
                .object(consumer)
                .unwrap()
                .interpreter()
                .unwrap()
                .stack_bytes(),
            0xdeadbeefu32.to_le_bytes()
        );
        runtime.update(2, 1).unwrap();
        assert!(runtime.host().raw_buffer(copied, 1).is_err());
        let vm = runtime.object(consumer).unwrap().interpreter().unwrap();
        let empty = u32::from_le_bytes(vm.read(12, 4).unwrap()[..].try_into().unwrap());
        assert_ne!(empty, 0);
        assert!(runtime.host().raw_buffer(empty, 0).is_err());
        assert_eq!(vm.stack_bytes(), 0xdeadbeefu32.to_le_bytes());
    }
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm =
        Interpreter::new(Program::from_code(&[0x3b], vec![0; 8], 16).unwrap(), vec![]).unwrap();
    let NativeReturn::Value(raw) = host.call(833, &8u32.to_le_bytes(), &mut vm).unwrap() else {
        panic!()
    };
    assert!(vm.read(raw, 1).is_err());
    assert!(host.call(834, &(raw + 1).to_le_bytes(), &mut vm).is_err());
    assert!(host.call(833, &u32::MAX.to_le_bytes(), &mut vm).is_err());
    assert_eq!(host.raw_buffer(raw, 8).unwrap(), [0; 8]);
}
