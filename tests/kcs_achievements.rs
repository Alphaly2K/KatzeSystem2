use katze_system2::{
    Result,
    expression::State,
    fes,
    kcs::{AchievementServices, CoreHost, Host, Interpreter, NativeReturn, OrdinaryTask, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, fes::Scene, kind},
};
use std::{cell::RefCell, rc::Rc};

type Calls = Rc<RefCell<Vec<(u32, bool, Vec<u8>)>>>;
struct Services(Calls);
impl AchievementServices for Services {
    fn unlock(&mut self, task_id: u32, name: &[u8]) -> Result<()> {
        self.0.borrow_mut().push((task_id, false, name.to_vec()));
        Ok(())
    }
    fn is_unlocked(&mut self, task_id: u32, name: &[u8]) -> Result<bool> {
        self.0.borrow_mut().push((task_id, true, name.to_vec()));
        Ok(name == b"demo7")
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for value in arguments {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn achievement_natives_format_before_optional_services_and_keep_vm_returns() -> Result<()> {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x1357_9bdf]);
    for (slot, address) in [128, 0x8000_0000].into_iter().enumerate() {
        for index in [1042, 1043] {
            instruction(&mut code, 0x25, &[address]);
            instruction(&mut code, 0x2b, &[index, 4]);
        }
        instruction(&mut code, 0x26, &[slot as u32 * 4]);
    }
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[32..36].copy_from_slice(&7u32.to_le_bytes());
    let source = b"demo%u[G32]%c[G36]ignored\0";
    data[128..128 + source.len()].copy_from_slice(source);
    let external = b"\x82\xa0\\n%u[G32]\0".to_vec();
    let program = Program::from_code(&code, data.clone(), 64)?;
    for available in [false, true] {
        for budget in [1, 100] {
            let mut task = OrdinaryTask::new(program.clone(), external.clone())?;
            task.task_id = 19;
            let mut state = State::new(0);
            let mut host = CoreHost::new(&mut state);
            let calls = Calls::default();
            if available {
                host.achievement_services = Some(Box::new(Services(calls.clone())));
            }
            let mut done = false;
            for _ in 0..32 {
                done = task.update(0, 1, &mut host, budget)?;
                if done {
                    break;
                }
            }
            assert!(done);
            assert_eq!(
                task.interpreter.read(0, 8)?,
                [u32::from(available).to_le_bytes(), [0; 4]].concat()
            );
            assert_eq!(task.interpreter.read(8, 248)?, &data[8..]);
            assert_eq!(
                task.interpreter.read(0x8000_0000, external.len())?,
                external
            );
            assert_eq!(task.interpreter.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
            let expected = if available {
                vec![
                    (19, false, b"demo7".to_vec()),
                    (19, true, b"demo7".to_vec()),
                    (19, false, b"\x82\xa0\\n7".to_vec()),
                    (19, true, b"\x82\xa0\\n7".to_vec()),
                ]
            } else {
                Vec::new()
            };
            assert_eq!(*calls.borrow(), expected);
            assert!(host.debug_messages.is_empty());
        }
    }
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0; 8], 0)?, vec![])?;
    for index in [1042, 1043] {
        assert!(host.call(index, &u32::MAX.to_le_bytes(), &mut vm).is_err());
        assert!(host.call(index, &[], &mut vm).is_err());
    }
    assert_eq!(
        host.call(1042, &0u32.to_le_bytes(), &mut vm)?,
        NativeReturn::Continue
    );
    assert_eq!(
        host.call(1043, &0u32.to_le_bytes(), &mut vm)?,
        NativeReturn::Value(0)
    );
    Ok(())
}

#[test]
fn achievement_management_shares_one_initialization_and_keeps_reset_side_effects() -> Result<()> {
    #[derive(Default)]
    struct Record {
        calls: Vec<(&'static str, u32)>,
        unlocked: bool,
    }
    struct Lifecycle(Rc<RefCell<Record>>);
    impl AchievementServices for Lifecycle {
        fn initialize(&mut self, task_id: u32) -> Result<()> {
            self.0.borrow_mut().calls.push(("initialize", task_id));
            Ok(())
        }
        fn unlock(&mut self, _task_id: u32, _name: &[u8]) -> Result<()> {
            self.0.borrow_mut().unlocked = true;
            Ok(())
        }
        fn is_unlocked(&mut self, _task_id: u32, _name: &[u8]) -> Result<bool> {
            Ok(self.0.borrow().unlocked)
        }
        fn reset(&mut self, task_id: u32) -> Result<()> {
            let mut record = self.0.borrow_mut();
            record.calls.push(("reset", task_id));
            record.unlocked = false;
            Ok(())
        }
    }
    struct Source;
    impl ProgramLoader for Source {
        fn load(&mut self, _name: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            for index in [1041, 1041] {
                instruction(&mut code, 0x2b, &[index, 0]);
            }
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, vec![], 32)?))
        }
    }

    let record = Rc::new(RefCell::new(Record {
        unlocked: true,
        ..Record::default()
    }));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        KcsFactory::new(Source),
    );
    runtime.set_budget(1, 1024);
    runtime.set_achievement_services(Lifecycle(record.clone()));
    let writer = runtime.create(0, kind::ORDINARY, b"writer", b"")?;
    runtime.update(1, 1)?;
    let mut scene = Scene::new(
        fes::Program::from_text(b"#START\nsteam_init\nsteam_achievement_reset\n$1=7\nwait 100\n")?,
        runtime.host().handle_allocator(),
    )?;
    let mut done = false;
    for _ in 0..32 {
        done = runtime.update_fes_scene(0, &mut scene, 2)?;
        if done {
            break;
        }
    }
    assert!(done);
    assert_eq!(runtime.numbers().shared[1], 7);
    assert_eq!(
        record.borrow().calls,
        [("initialize", writer), ("reset", 0)]
    );
    assert!(!record.borrow().unlocked);

    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    host.task_id = 19;
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![0; 8], 0)?, vec![])?;
    assert_eq!(host.call(1041, &[], &mut vm)?, NativeReturn::Continue);
    assert!(
        host.call(1044, &[], &mut vm)
            .unwrap_err()
            .message
            .contains("未初始化返回值")
    );
    host.set_achievement_services(Lifecycle(record.clone()));
    assert!(host.call(1041, &[0; 4], &mut vm).is_err());
    assert_eq!(host.call(1041, &[], &mut vm)?, NativeReturn::Continue);
    record.borrow_mut().unlocked = true;
    assert!(host.call(1044, &[0; 4], &mut vm).is_err());
    assert!(record.borrow().unlocked);
    assert!(
        host.call(1044, &[], &mut vm)
            .unwrap_err()
            .message
            .contains("未初始化返回值")
    );
    assert_eq!(host.call(1043, &[0; 4], &mut vm)?, NativeReturn::Value(0));
    assert_eq!(host.call(1041, &[], &mut vm)?, NativeReturn::Continue);
    assert_eq!(
        record.borrow().calls,
        [
            ("initialize", writer),
            ("reset", 0),
            ("initialize", 19),
            ("reset", 19)
        ]
    );
    Ok(())
}
