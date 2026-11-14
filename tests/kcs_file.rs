use katze_system2::{
    Error, Result,
    expression::State,
    io::FileServices,
    kcs::{CoreHost, Host, Interpreter, OrdinaryTask, Program},
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
