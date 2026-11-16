use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{
        CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program, SystemHost,
        SystemServices,
    },
};

#[derive(Clone, Debug, PartialEq)]
struct Call {
    task_id: u32,
    target: Vec<u8>,
    parameters: Vec<u8>,
    directory: Vec<u8>,
}

#[test]
fn window_content_query_uses_host_bounds_and_preserves_aliased_write_order() {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    host.surface_size = [1280, 720];
    let mut vm = Interpreter::new(
        Program::from_code(&[0x3b], vec![0; 32], 16).unwrap(),
        vec![],
    )
    .unwrap();
    let call = |host: &mut CoreHost<'_>, vm: &mut Interpreter, addresses: [u32; 4]| {
        host.call(
            787,
            &addresses
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            vm,
        )
    };
    let words = |vm: &Interpreter| {
        vm.read(0, 16)
            .unwrap()
            .chunks_exact(4)
            .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        call(&mut host, &mut vm, [0, 4, 8, 12]).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(words(&vm), [0, 0, 1280, 720]);
    host.window_content_rect = Some(katze_system2::graphics::Rect {
        left: -20,
        top: 33,
        right: 1004,
        bottom: 609,
    });
    call(&mut host, &mut vm, [0, 4, 8, 12]).unwrap();
    assert_eq!(words(&vm), [-20, 33, 1024, 576]);
    call(&mut host, &mut vm, [0; 4]).unwrap();
    assert_eq!(words(&vm)[0], 576);
    let before = vm.read(0, 16).unwrap().to_vec();
    assert!(call(&mut host, &mut vm, [0, 4, 8, u32::MAX]).is_err());
    assert_eq!(vm.read(0, 16).unwrap(), before);
}

#[derive(Default)]
struct Services(Vec<Call>);

impl SystemServices for Services {
    fn open(
        &mut self,
        task_id: u32,
        target: &[u8],
        parameters: &[u8],
        directory: &[u8],
    ) -> Result<()> {
        self.0.push(Call {
            task_id,
            target: target.to_vec(),
            parameters: parameters.to_vec(),
            directory: directory.to_vec(),
        });
        if self.0.len() == 2 {
            return Err(Error::new(0, "合成系统服务错误"));
        }
        Ok(())
    }
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

#[test]
fn system_open_formats_each_field_and_completes_the_service_before_vm_continuation() {
    let mut code = Vec::new();
    for call in 0..2 {
        for address in [64, 128, 192] {
            ins(&mut code, 0x25, &[address]);
        }
        ins(&mut code, 0x2b, &[629, 12]);
        for (native, target) in [(68, 0), (5, 4)] {
            ins(&mut code, 0x2b, &[native, 0]);
            ins(&mut code, 0x26, &[target + call * 8]);
        }
    }
    code.push(0x3b);
    let mut data = vec![0; 256];
    for (offset, bytes) in [
        (12, 17u32.to_le_bytes().as_slice()),
        (32, b"nested%d[G12]\0"),
        (64, b"demo%c[G16]tail\0"),
        (128, b"%s[G32] %d[H0]\0"),
        (192, b"\x82\xa0\\\0"),
    ] {
        data[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    let mut task = OrdinaryTask::new(
        Program::from_code(&code, data, 128).unwrap(),
        (-1i32).to_le_bytes().to_vec(),
    )
    .unwrap();
    task.task_id = 42;
    task.clock.origin = 10;
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut services = Services::default();
    let mut host = SystemHost::new(&mut core, &mut services);
    let mut error = None;
    for _ in 0..100 {
        match task.update(100, 1, &mut host, 1) {
            Ok(false) => {}
            result => {
                error = Some(result.unwrap_err());
                break;
            }
        }
    }
    assert_eq!(error.unwrap().message, "合成系统服务错误");
    assert_eq!(
        task.interpreter.read(0, 12).unwrap(),
        [42u32.to_le_bytes(), 90u32.to_le_bytes(), [0; 4]].concat()
    );
    assert!(task.interpreter.stack_bytes().is_empty());
    let expected = Call {
        task_id: 42,
        target: b"demo".to_vec(),
        parameters: b"nested17 -1".to_vec(),
        directory: b"\x82\xa0\\".to_vec(),
    };
    assert_eq!(services.0, [expected.clone(), expected]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DialogCall(u32, Vec<u16>);
#[derive(Clone, Default)]
struct Dialogs(std::rc::Rc<std::cell::RefCell<Vec<DialogCall>>>);
impl SystemServices for Dialogs {
    fn message_box(&mut self, task_id: u32, text: &[u16]) -> Result<()> {
        self.0.borrow_mut().push(DialogCall(task_id, text.to_vec()));
        Ok(())
    }
}

#[test]
fn message_box_services_keep_cp932_literals_and_void_returns_in_both_hosts() -> Result<()> {
    fn finish(task: &mut OrdinaryTask, host: &mut impl Host, budget: usize) -> Result<()> {
        for _ in 0..32 {
            if task.update(0, 1, host, budget)? {
                return Ok(());
            }
        }
        panic!("合成任务未完成")
    }
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0x1357_9bdf]);
    for address in [128, 0x8000_0000] {
        ins(&mut code, 0x25, &[address]);
        ins(&mut code, 0x2b, &[445, 4]);
    }
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[32..36].copy_from_slice(&7u32.to_le_bytes());
    let source = b"\x82\xa0\\n%u[G32]%% %c[G36]ignored\0";
    data[128..128 + source.len()].copy_from_slice(source);
    let external = b"%c[G36]ignored\0".to_vec();
    let program = Program::from_code(&code, data.clone(), 64)?;
    for wrapped in [false, true] {
        for budget in [1, 100] {
            let mut task = OrdinaryTask::new(program.clone(), external.clone())?;
            task.task_id = 42;
            let mut state = State::new(0);
            let mut core = CoreHost::new(&mut state);
            let mut dialogs = Dialogs::default();
            if wrapped {
                let mut host = SystemHost::new(&mut core, &mut dialogs);
                finish(&mut task, &mut host, budget)?;
            } else {
                core.system_services = Some(Box::new(dialogs.clone()));
                finish(&mut task, &mut core, budget)?;
            }
            assert_eq!(
                *dialogs.0.borrow(),
                [
                    DialogCall(42, vec![0x3042, 92, 110, 55, 37, 32]),
                    DialogCall(42, vec![])
                ]
            );
            assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
            assert_eq!(task.interpreter.read(0, 256)?, data);
            assert_eq!(
                task.interpreter.read(0x8000_0000, external.len())?,
                external
            );
        }
    }
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let dialogs = Dialogs::default();
    host.system_services = Some(Box::new(dialogs.clone()));
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0; 4096], 0)?, vec![])?;
    vm.write(0, &[vec![b'x'; 1023], vec![0]].concat())?;
    assert_eq!(
        host.call(445, &0u32.to_le_bytes(), &mut vm)?,
        NativeReturn::Continue
    );
    assert_eq!(dialogs.0.borrow()[0].1, vec![u16::from(b'x'); 1023]);
    vm.write(0, &[vec![b'x'; 1024], vec![0]].concat())?;
    assert!(host.call(445, &0u32.to_le_bytes(), &mut vm).is_err());
    assert!(host.call(445, &[], &mut vm).is_err());
    assert_eq!(dialogs.0.borrow().len(), 1);
    host.system_services = None;
    vm.write(0, &[0])?;
    assert!(host.call(445, &0u32.to_le_bytes(), &mut vm).is_err());
    Ok(())
}
