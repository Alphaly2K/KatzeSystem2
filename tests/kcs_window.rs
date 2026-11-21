use katze_system2::{
    Result,
    expression::State,
    host::platform::WindowAction,
    kcs::{CoreHost, Host, HostState, Interpreter, NativeReturn, OrdinaryTask, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

fn ins(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
    code.push(opcode);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn queued_window_actions_share_debug_mode_and_preserve_void_stack_across_hosts() -> Result<()> {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0x1357_9bdf]);
    for (index, mode) in [
        (628, Some(0)),
        (845, None),
        (457, None),
        (628, Some(u32::MAX)),
        (846, None),
        (847, None),
    ] {
        if let Some(mode) = mode {
            ins(&mut code, 0x25, &[mode]);
        }
        ins(
            &mut code,
            0x2b,
            &[index, if mode.is_some() { 4 } else { 0 }],
        );
    }
    ins(&mut code, 0x2b, &[465, 0]);
    ins(&mut code, 0x26, &[0]);
    ins(&mut code, 0x26, &[4]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 32], 64)?;
    let check = |host: &HostState, vm: &Interpreter, task_id: u32| -> Result<()> {
        assert_eq!(host.debug_mode, u32::MAX);
        assert_eq!(
            vm.read(0, 8)?,
            [u32::MAX.to_le_bytes(), 0x1357_9bdfu32.to_le_bytes()].concat()
        );
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(
            host.window_requests
                .iter()
                .map(|r| (r.task_id, r.action))
                .collect::<Vec<_>>(),
            [
                WindowAction::RebuildMenu,
                WindowAction::ToggleFullscreen,
                WindowAction::OpenSystemProperties,
                WindowAction::RebuildMenu,
                WindowAction::Minimize,
                WindowAction::CloseFromMenu,
            ]
            .map(|action| (task_id, action))
        );
        assert!(host.window_close_requests.is_empty());
        Ok(())
    };
    for budget in [1, 100] {
        let mut state = State::new(0);
        let mut core = CoreHost::new(&mut state);
        let mut task = OrdinaryTask::new(program.clone(), vec![])?;
        task.task_id = 19;
        let mut finished = false;
        for _ in 0..100 {
            finished = task.update(0, 1, &mut core, budget)?;
            if finished {
                break;
            }
        }
        assert!(finished);
        check(&core.host, &task.interpreter, 19)?;

        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        rt.set_budget(budget, 4096);
        let id = rt.create(0, kind::ORDINARY, b"window", b"")?;
        for _ in 0..100 {
            rt.update(0, 1)?;
            if rt.host().window_requests.len() == 6
                && rt.object(id).unwrap().interpreter().unwrap().pc == code.len()
            {
                break;
            }
        }
        check(rt.host(), rt.object(id).unwrap().interpreter().unwrap(), id)?;
    }
    Ok(())
}

#[test]
fn invalid_window_arguments_do_not_change_mode_or_queue() -> Result<()> {
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    host.debug_mode = 7;
    let mut vm = Interpreter::new(Program::from_code(&[], vec![], 0)?, vec![])?;
    for (index, arguments) in [
        (457, vec![0; 4]),
        (628, vec![]),
        (628, vec![0; 8]),
        (845, vec![0; 4]),
        (846, vec![0; 4]),
        (847, vec![0; 4]),
    ] {
        assert!(host.call(index, &arguments, &mut vm).is_err());
        assert_eq!(host.debug_mode, 7);
        assert!(host.window_requests.is_empty());
    }
    assert_eq!(
        host.call(628, &u32::MAX.to_le_bytes(), &mut vm)?,
        NativeReturn::Continue
    );
    assert_eq!(host.call(465, &[], &mut vm)?, NativeReturn::Value(u32::MAX));
    Ok(())
}
