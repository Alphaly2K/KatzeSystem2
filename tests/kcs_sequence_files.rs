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
    let program = Program::from_code(&code, data, 64).unwrap();
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
            bytes: Rc::new(RefCell::new(Some(b"CGL\0payload".to_vec()))),
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
            b"payload"
        );
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(12, 12).unwrap(),
            [1u32; 3]
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
