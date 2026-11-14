use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, ResourceHost, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Requests = Rc<RefCell<Vec<(Vec<u8>, Vec<u8>)>>>;

struct Files {
    bytes: Rc<RefCell<Option<Vec<u8>>>>,
    fail: Rc<Cell<bool>>,
    requests: Requests,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.requests
            .borrow_mut()
            .push((category.to_vec(), name.to_vec()));
        Ok(b"config/data".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"config/data");
        assert_eq!(mode, ResourceMode::LooseFirst);
        if self.fail.get() {
            return Err(Error::new(0, "合成资源读取失败"));
        }
        Ok(self.bytes.borrow().clone())
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

fn grouped_file(stride: usize) -> Vec<u8> {
    fn block(data: &mut Vec<u8>, stride: usize, name: &[u8], records: &[&[u8]]) {
        let start = data.len();
        data.resize(start + 72 + stride * records.len(), 0);
        data[start..start + 4]
            .copy_from_slice(&((72 + stride * records.len()) as u32).to_le_bytes());
        data[start + 4..start + 4 + name.len()].copy_from_slice(name);
        data[start + 68..start + 72].copy_from_slice(&(records.len() as u32).to_le_bytes());
        for (index, record) in records.iter().enumerate() {
            let offset = start + 72 + index * stride;
            data[offset..offset + record.len()].copy_from_slice(record);
        }
    }
    let mut data = vec![0; 12];
    data[4..8].copy_from_slice(&42u32.to_le_bytes());
    data[8..12].copy_from_slice(&2u32.to_le_bytes());
    block(
        &mut data,
        stride,
        b"Group",
        &[
            b"alpha,one,1,2",
            b"beta,two,-1",
            b"alpha,one,1",
            b"Solo",
            b"cgL,x,5",
            b",",
        ],
    );
    let second = data.len();
    block(&mut data, stride, b"GROUP", &[b"later,only,9"]);
    data[second..second + 4].fill(0);
    let next = data.len();
    data[..4].copy_from_slice(&(next as u32).to_le_bytes());
    data.extend([0; 12]);
    data[next + 4..next + 8].copy_from_slice(&7u32.to_le_bytes());
    data[next + 8..next + 12].copy_from_slice(&1u32.to_le_bytes());
    block(&mut data, stride, b"other", &[b"gamma,three,7"]);
    data[next + 12..next + 16].fill(0);
    if stride == 256 {
        data.splice(..0, *b"CGL\0");
    }
    data
}

#[test]
fn sequence_group_imports_merge_first_matching_records_and_keep_completed_reads_on_truncation() {
    for stride in [64, 256] {
        let mut state = State::new(0);
        let mut core = CoreHost::new(&mut state);
        core.sequence_name_mode = 1;
        let mut vm =
            Interpreter::new(Program::from_code(&[], vec![0; 1024], 32).unwrap(), vec![]).unwrap();
        let NativeReturn::Value(handle) = call(&mut core, &mut vm, 419, &[]).unwrap() else {
            panic!()
        };
        let bytes = Rc::new(RefCell::new(Some(grouped_file(stride))));
        let mut files = Files {
            bytes: bytes.clone(),
            fail: Rc::new(Cell::new(false)),
            requests: Requests::default(),
        };
        let mut host = ResourceHost::new(&mut core, &mut files);
        vm.write(128, b"data\0").unwrap();
        call(&mut host, &mut vm, 458, &[handle, 128]).unwrap();
        vm.write(256, b"seed,name,6\0").unwrap();
        call(&mut host, &mut vm, 422, &[handle, 256]).unwrap();
        vm.write(128, b"gRoUp\0").unwrap();
        let original = vm.read(0, 1024).unwrap().to_vec();
        for _ in 0..2 {
            assert_eq!(
                call(&mut host, &mut vm, 459, &[handle, 42, 128]).unwrap(),
                NativeReturn::Continue
            );
            assert_eq!(vm.read(0, 1024).unwrap(), original);
        }
        for (query, expected) in [
            (b"seed,name,6".as_slice(), 1),
            (b"alpha,one,1", 1),
            (b"alpha,one,1,2", 1),
            (b"beta,two,-7", 1),
            (b"Solo", 1),
            (b"cgL,x,5", 1),
            (b"cgm,x,5", 0),
            (b"later,only,9", 0),
            (b"gamma,three,7", 0),
        ] {
            vm.write(320, query).unwrap();
            vm.write(320 + query.len() as u32, &[0]).unwrap();
            assert_eq!(
                call(&mut host, &mut vm, 423, &[handle, 320]).unwrap(),
                NativeReturn::Value(expected)
            );
        }
        vm.write(128, b"OTHER\0").unwrap();
        call(&mut host, &mut vm, 459, &[handle, 7, 128]).unwrap();
        vm.write(320, b"gamma,three,7\0").unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 423, &[handle, 320]).unwrap(),
            NativeReturn::Value(1)
        );
        for args in [vec![0], vec![0, 42, u32::MAX]] {
            assert_eq!(
                call(&mut host, &mut vm, 459, &args).unwrap(),
                NativeReturn::Continue
            );
        }
        let mut truncated = grouped_file(stride);
        truncated.truncate(usize::from(stride == 256) * 4 + 84 + stride * 5);
        *bytes.borrow_mut() = Some(truncated);
        call(&mut host, &mut vm, 458, &[handle, 128]).unwrap();
        call(&mut host, &mut vm, 421, &[handle]).unwrap();
        vm.write(128, b"Group\0").unwrap();
        assert!(call(&mut host, &mut vm, 459, &[handle, 42, 128]).is_err());
        vm.write(320, b"alpha,one,1\0").unwrap();
        assert_eq!(
            call(&mut host, &mut vm, 423, &[handle, 320]).unwrap(),
            NativeReturn::Value(1)
        );
    }
}

#[test]
fn sequence_file_replacement_keeps_names_and_clear_keeps_file_data() {
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 1024], 32).unwrap(), vec![]).unwrap();
    let NativeReturn::Value(handle) = call(&mut core, &mut vm, 419, &[]).unwrap() else {
        panic!()
    };
    vm.write(0, &17u32.to_le_bytes()).unwrap();
    vm.write(128, b"piece%d[G0]\0").unwrap();
    vm.write(256, b"foo,bar,1\0").unwrap();
    let bytes = Rc::new(RefCell::new(Some(b"CGL\0ABC".to_vec())));
    let fail = Rc::new(Cell::new(false));
    let requests = Requests::default();
    let mut files = Files {
        bytes: bytes.clone(),
        fail: fail.clone(),
        requests: requests.clone(),
    };
    let mut host = ResourceHost::new(&mut core, &mut files);
    call(&mut host, &mut vm, 422, &[handle, 256]).unwrap();
    let original = vm.read(0, 1024).unwrap().to_vec();
    assert_eq!(
        call(&mut host, &mut vm, 458, &[handle, 128]).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(
        *requests.borrow(),
        [(b"config".to_vec(), b"piece17".to_vec())]
    );
    assert_eq!(host.inner.sequence_data(handle).unwrap().bytes(), b"ABC");
    assert_eq!(
        host.inner.sequence_data(handle).unwrap().record_stride(),
        256
    );
    assert_eq!(
        call(&mut host, &mut vm, 423, &[handle, 256]).unwrap(),
        NativeReturn::Value(1)
    );
    let NativeReturn::Value(container) = call(&mut host, &mut vm, 238, &[77]).unwrap() else {
        panic!()
    };
    assert_eq!(
        call(&mut host, &mut vm, 424, &[container, 7, handle]).unwrap(),
        NativeReturn::Value(1)
    );
    call(&mut host, &mut vm, 421, &[handle]).unwrap();
    assert_eq!(host.inner.sequence_data(handle).unwrap().bytes(), b"ABC");
    assert_eq!(
        call(&mut host, &mut vm, 423, &[handle, 256]).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(
        call(&mut host, &mut vm, 425, &[container, 7, handle]).unwrap(),
        NativeReturn::Value(1)
    );
    call(&mut host, &mut vm, 246, &[container]).unwrap();
    assert_eq!(host.inner.sequence_data(handle).unwrap().bytes(), b"ABC");
    for missing in [None, Some(Vec::new())] {
        *bytes.borrow_mut() = missing;
        call(&mut host, &mut vm, 458, &[handle, 128]).unwrap();
        assert_eq!(host.inner.sequence_data(handle).unwrap().bytes(), b"ABC");
    }
    fail.set(true);
    assert!(call(&mut host, &mut vm, 458, &[handle, 128]).is_err());
    assert_eq!(host.inner.sequence_data(handle).unwrap().bytes(), b"ABC");
    fail.set(false);
    for (file, data, stride) in [
        (b"CGL\0".as_slice(), b"CGL\0".as_slice(), 64),
        (b"legacy\0bytes", b"legacy\0bytes", 64),
        (b"CGL\0XYZ", b"XYZ", 256),
    ] {
        *bytes.borrow_mut() = Some(file.to_vec());
        call(&mut host, &mut vm, 458, &[handle, 128]).unwrap();
        let loaded = host.inner.sequence_data(handle).unwrap();
        assert_eq!((loaded.bytes(), loaded.record_stride()), (data, stride));
        assert_eq!(
            call(&mut host, &mut vm, 423, &[handle, 256]).unwrap(),
            NativeReturn::Value(1)
        );
    }
    let count = requests.borrow().len();
    for args in [vec![0], vec![0, u32::MAX]] {
        assert_eq!(
            call(&mut host, &mut vm, 458, &args).unwrap(),
            NativeReturn::Continue
        );
    }
    assert_eq!(requests.borrow().len(), count);
    *bytes.borrow_mut() = Some(b"CGL\0empty-name".to_vec());
    call(&mut host, &mut vm, 458, &[handle, 400]).unwrap();
    assert_eq!(
        requests.borrow().last(),
        Some(&(b"config".to_vec(), Vec::new()))
    );
    assert_eq!(
        host.inner.sequence_data(handle).unwrap().bytes(),
        b"empty-name"
    );
    call(&mut host, &mut vm, 458, &[handle, 0]).unwrap();
    assert_eq!(
        requests.borrow().last(),
        Some(&(b"config".to_vec(), vec![17]))
    );
    assert_eq!(vm.read(0, 1024).unwrap(), original);
    assert!(vm.stack_bytes().is_empty());
    call(&mut host, &mut vm, 420, &[handle]).unwrap();
    assert!(host.inner.sequence_data(handle).is_err());
    assert!(call(&mut host, &mut vm, 458, &[handle, 128]).is_err());
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[(u8, u32)]) {
    code.push(op);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn runtime_script_roundtrips_sequence_tree_and_keeps_file_data_across_budget_continuations() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0)]);
    ins(&mut code, 0x2b, &[(3, 458), (3, 4)]);
    ins(&mut code, 0x2b, &[(3, 419), (3, 0)]);
    ins(&mut code, 0x26, &[(3, 0)]);
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x25, &[(3, 128)]);
    ins(&mut code, 0x2b, &[(3, 458), (3, 8)]);
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x25, &[(3, 42)]);
    ins(&mut code, 0x25, &[(3, 192)]);
    ins(&mut code, 0x2b, &[(3, 459), (3, 12)]);
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x25, &[(3, 160)]);
    ins(&mut code, 0x2b, &[(3, 422), (3, 8)]);
    ins(&mut code, 0x25, &[(3, 77)]);
    ins(&mut code, 0x2b, &[(3, 238), (3, 4)]);
    ins(&mut code, 0x26, &[(3, 8)]);
    for index in [424, 425] {
        ins(&mut code, 0x25, &[(0x13, 8)]);
        ins(&mut code, 0x25, &[(3, 7)]);
        ins(&mut code, 0x25, &[(0x13, 0)]);
        ins(&mut code, 0x2b, &[(3, index), (3, 12)]);
        ins(&mut code, 0x26, &[(3, if index == 424 { 12 } else { 20 })]);
        if index == 424 {
            ins(&mut code, 0x25, &[(0x13, 0)]);
            ins(&mut code, 0x2b, &[(3, 421), (3, 4)]);
        }
    }
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x25, &[(3, 160)]);
    ins(&mut code, 0x2b, &[(3, 423), (3, 8)]);
    ins(&mut code, 0x26, &[(3, 16)]);
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x25, &[(3, 224)]);
    ins(&mut code, 0x2b, &[(3, 423), (3, 8)]);
    ins(&mut code, 0x26, &[(3, 24)]);
    ins(&mut code, 0x25, &[(0x13, 8)]);
    ins(&mut code, 0x2b, &[(3, 246), (3, 4)]);
    ins(&mut code, 0x2b, &[(3, 65), (3, 0)]);
    ins(&mut code, 0x25, &[(0x13, 0)]);
    ins(&mut code, 0x2b, &[(3, 420), (3, 4)]);
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[16..20].copy_from_slice(&7u32.to_le_bytes());
    let name = b"item%d[G16]\0";
    data[128..128 + name.len()].copy_from_slice(name);
    data[160..166].copy_from_slice(b"a,b,1\0");
    data[192..198].copy_from_slice(b"GROUP\0");
    data[224..236].copy_from_slice(b"alpha,one,1\0");
    let program = Program::from_code(&code, data, 64).unwrap();
    let file = grouped_file(256);
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(8),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 10000);
        runtime.set_resources(Files {
            bytes: Rc::new(RefCell::new(Some(file.clone()))),
            fail: Rc::new(Cell::new(false)),
            requests: requests.clone(),
        });
        let task = runtime.create(0, kind::ORDINARY, b"script", b"").unwrap();
        runtime.update(10, 1).unwrap();
        let handle = u32::from_le_bytes(
            runtime
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap()
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            runtime.host().sequence_data(handle).unwrap().bytes(),
            &file[4..]
        );
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(12, 16).unwrap(),
            [1u32; 4]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            *requests.borrow(),
            [(b"config".to_vec(), b"item7".to_vec())]
        );
        assert!(
            runtime
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .stack_bytes()
                .is_empty()
        );
        runtime.update(20, 1).unwrap();
        assert!(runtime.host().sequence_data(handle).is_err());
    }
}
