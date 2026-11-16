use katze_system2::{
    Error, Result,
    expression::State,
    io::FileServices,
    kcs::{
        CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program, ResourceHost,
        ResourceServices,
    },
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Debug, PartialEq)]
struct Line {
    owner: u32,
    name: Vec<u8>,
    text: Vec<u8>,
}
struct Files(Rc<RefCell<Vec<Line>>>);
impl FileServices for Files {
    fn append_line(&mut self, owner: u32, name: &[u8], text: &[u8]) -> Result<()> {
        self.0.borrow_mut().push(Line {
            owner,
            name: name.to_vec(),
            text: text.to_vec(),
        });
        Ok(())
    }
}
fn instruction(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
#[test]
fn file_append_preserves_raw_names_expands_only_body_and_keeps_the_return_stack() {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x12345678]);
    for address in [128, 192] {
        instruction(&mut code, 0x25, &[64]);
        instruction(&mut code, 0x25, &[address]);
        instruction(&mut code, 0x2b, &[567, 8]);
    }
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[12..16].copy_from_slice(&17u32.to_le_bytes());
    let name = b"\x83\x65\x83\x58\x83\x67\\%d[G12].log\0";
    for (at, bytes) in [
        (64, name.as_slice()),
        (128, b"value=%d[G12] %d[H0]\0".as_slice()),
        (192, b"%c[G16]hidden\0".as_slice()),
    ] {
        data[at..at + bytes.len()].copy_from_slice(bytes);
    }
    for budget in [1, 4096] {
        let mut task = OrdinaryTask::new(
            Program::from_code(&code, data.clone(), 128).unwrap(),
            (-1i32).to_le_bytes().to_vec(),
        )
        .unwrap();
        task.task_id = 42;
        let mut state = State::new(0);
        let mut host = CoreHost::new(&mut state);
        let lines = Rc::new(RefCell::new(Vec::new()));
        host.file_services = Some(Box::new(Files(lines.clone())));
        let mut finished = false;
        for _ in 0..100 {
            if task.update(100, 1, &mut host, budget).unwrap() {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(task.interpreter.stack_bytes(), 0x12345678u32.to_le_bytes());
        assert_eq!(task.interpreter.read(0, data.len()).unwrap(), data);
        let expected_name = name[..name.len() - 1].to_vec();
        assert_eq!(
            *lines.borrow(),
            [
                Line {
                    owner: 42,
                    name: expected_name.clone(),
                    text: b"value=17 -1".to_vec()
                },
                Line {
                    owner: 42,
                    name: expected_name,
                    text: Vec::new()
                }
            ]
        );
    }
    let mut vm = Interpreter::new(
        Program::from_code(&[0x3b], data, 16).unwrap(),
        (-1i32).to_le_bytes().to_vec(),
    )
    .unwrap();
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let error = host
        .call(
            567,
            &[64u32.to_le_bytes(), 128u32.to_le_bytes()].concat(),
            &mut vm,
        )
        .unwrap_err();
    assert_eq!(error, Error::new(0, "宿主未配置文件写入服务"));
}

#[derive(Default)]
struct Transfers {
    reads: Vec<(Vec<u8>, ResourceMode)>,
    creates: Vec<(u32, Vec<u8>)>,
    written: Vec<u8>,
    fail_write: bool,
    closed: usize,
}

#[derive(Clone)]
struct TransferFiles(Rc<RefCell<Transfers>>);

impl ResourceServices for TransferFiles {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.0.borrow_mut().reads.push((name.to_vec(), mode));
        Ok((name == b"file7").then(|| b"abcde".to_vec()))
    }
}

struct Output(Rc<RefCell<Transfers>>);
impl std::io::Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut state = self.0.borrow_mut();
        if state.fail_write {
            return Err(std::io::Error::other("write failed"));
        }
        let count = bytes.len().min(2);
        state.written.extend(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.0.borrow_mut().closed += 1;
    }
}
impl FileServices for TransferFiles {
    fn create(&mut self, owner: u32, name: &[u8]) -> Result<Option<Box<dyn std::io::Write>>> {
        let mut state = self.0.borrow_mut();
        state.creates.push((owner, name.to_vec()));
        if name != b"out7" {
            return Ok(None);
        }
        state.written.clear();
        Ok(Some(Box::new(Output(self.0.clone()))))
    }
    fn append_line(&mut self, _: u32, _: &[u8], _: &[u8]) -> Result<()> {
        unreachable!()
    }
}

fn transfer_data() -> Vec<u8> {
    let mut data = vec![0xcc; 384];
    data[20..24].copy_from_slice(&7u32.to_le_bytes());
    for (at, text) in [
        (64, b"file%d[G20]\0".as_slice()),
        (128, b"rb\0"),
        (160, b"wr\0"),
        (192, b"out%d[G20]\0"),
        (224, b"a\0"),
    ] {
        data[at..at + text.len()].copy_from_slice(text);
    }
    data
}

fn native(host: &mut impl Host, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}
fn result(value: NativeReturn) -> u32 {
    let NativeReturn::Value(value) = value else {
        panic!("native value expected")
    };
    value
}

#[test]
fn file_handles_keep_independent_cursors_byte_counts_partial_writes_and_close_state() {
    let transfers = Rc::new(RefCell::new(Transfers::default()));
    let mut files = TransferFiles(transfers.clone());
    let mut state = State::new(0);
    let mut core = CoreHost::new(&mut state);
    core.task_id = 42;
    core.file_services = Some(Box::new(files.clone()));
    let mut vm = Interpreter::new(
        Program::from_code(&[], transfer_data(), 64).unwrap(),
        vec![0xcc; 384],
    )
    .unwrap();
    let mut host = ResourceHost::new(&mut core, &mut files);
    let first = result(native(&mut host, &mut vm, 29, &[64, 128]));
    let second = result(native(&mut host, &mut vm, 29, &[64, 224]));
    assert_ne!(first, second);
    assert_eq!(
        transfers.borrow().reads,
        [
            (b"file7".to_vec(), ResourceMode::File),
            (b"file7".to_vec(), ResourceMode::LooseFirst)
        ]
    );
    assert_eq!(result(native(&mut host, &mut vm, 29, &[192, 128])), 0);
    assert_eq!(
        result(native(&mut host, &mut vm, 30, &[256, 2, 3, first])),
        5
    );
    assert_eq!(vm.read(256, 6).unwrap().as_ref(), b"abcde\xcc");
    assert_eq!(
        result(native(&mut host, &mut vm, 30, &[256, 2, 3, first])),
        0
    );
    assert_eq!(
        result(native(&mut host, &mut vm, 30, &[0x80000100, 2, 1, second])),
        2
    );
    assert_eq!(vm.read(0x80000100, 3).unwrap().as_ref(), b"ab\xcc");
    let output = result(native(&mut host, &mut vm, 29, &[192, 160]));
    assert_eq!(transfers.borrow().creates, [(42, b"out7".to_vec())]);
    assert_eq!(
        result(native(&mut host, &mut vm, 31, &[256, 2, 3, output])),
        2
    );
    assert_eq!(
        result(native(&mut host, &mut vm, 31, &[256, 2, 3, second])),
        0
    );
    assert_eq!(
        result(native(
            &mut host,
            &mut vm,
            31,
            &[256, 0x80000000, 2, output]
        )),
        0
    );
    assert_eq!(
        result(native(
            &mut host,
            &mut vm,
            31,
            &[256, u32::MAX, u32::MAX, output]
        )),
        1
    );
    assert_eq!(transfers.borrow().written, b"aba");
    transfers.borrow_mut().fail_write = true;
    assert_eq!(
        result(native(&mut host, &mut vm, 31, &[256, 1, 5, output])),
        0
    );
    assert_eq!(
        result(native(&mut host, &mut vm, 30, &[256, 1, 5, output])),
        0
    );
    assert_eq!(
        result(native(
            &mut host,
            &mut vm,
            30,
            &[u32::MAX, u32::MAX, u32::MAX, 0]
        )),
        0
    );
    for handle in [first, second, output, 0] {
        assert_eq!(
            native(&mut host, &mut vm, 32, &[handle]),
            NativeReturn::Continue
        );
    }
    assert_eq!(transfers.borrow().closed, 1);
    assert!(host.call(32, &first.to_le_bytes(), &mut vm).is_err());
    assert!(
        host.call(
            30,
            &[256u32, 1, 1, first]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            &mut vm
        )
        .is_err()
    );
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn runtime_file_transfers_resume_once_across_small_budgets_and_release_the_writer() {
    let mut code = Vec::new();
    let push = |code: &mut Vec<u8>, value: u32, indirect: bool| {
        code.extend([0x25, if indirect { 0x13 } else { 3 }]);
        code.extend(value.to_le_bytes());
    };
    for (name, mode, handle, count, writing) in [(64, 128, 0, 4, false), (192, 160, 8, 12, true)] {
        push(&mut code, name, false);
        push(&mut code, mode, false);
        instruction(&mut code, 0x2b, &[29, 8]);
        instruction(&mut code, 0x26, &[handle]);
        push(&mut code, 256, false);
        push(&mut code, if writing { 1 } else { 2 }, false);
        push(&mut code, if writing { 4 } else { 3 }, writing);
        push(&mut code, handle, true);
        instruction(&mut code, 0x2b, &[if writing { 31 } else { 30 }, 16]);
        instruction(&mut code, 0x26, &[count]);
        push(&mut code, handle, true);
        instruction(&mut code, 0x2b, &[32, 4]);
    }
    code.push(0x3b);
    for budget in [1, 4096] {
        let transfers = Rc::new(RefCell::new(Transfers::default()));
        let files = TransferFiles(transfers.clone());
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(
                Program::from_code(&code, transfer_data(), 128).unwrap(),
            )),
        );
        runtime.set_budget(budget, 10000);
        runtime.set_resources(files.clone());
        runtime.set_file_services(files);
        let id = runtime.create(0, kind::ORDINARY, b"", b"").unwrap();
        for now in 1..100 {
            runtime.update(now, 1).unwrap();
        }
        let vm = runtime.object(id).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(4, 4).unwrap().as_ref(), 5u32.to_le_bytes());
        assert_eq!(vm.read(12, 4).unwrap().as_ref(), 2u32.to_le_bytes());
        assert_eq!(vm.read(256, 6).unwrap().as_ref(), b"abcde\xcc");
        assert!(vm.stack_bytes().is_empty());
        let transfers = transfers.borrow();
        assert_eq!(transfers.reads, [(b"file7".to_vec(), ResourceMode::File)]);
        assert_eq!(transfers.creates, [(id, b"out7".to_vec())]);
        assert_eq!(transfers.written, b"ab");
        assert_eq!(transfers.closed, 1);
    }
}
