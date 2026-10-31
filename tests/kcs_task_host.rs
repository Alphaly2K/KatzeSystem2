use katze_system2::{
    Error, Result, Step,
    expression::State,
    kcs::{
        CoreHost, Host, Interpreter, MemoryCopyMode, NativeReturn, OrdinaryTask, Program, TaskHost,
        TaskServices,
    },
};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

struct Services {
    target: Interpreter,
    queued: Vec<(u32, [u32; 4])>,
    dispatched: usize,
}

impl TaskServices for Services {
    fn dispatch_message(&mut self, target: u32, message: &mut [u32; 4]) -> Result<u32> {
        if target != 42 {
            return Err(Error::new(0, "任务不存在"));
        }
        self.dispatched += 1;
        let mut state = State::new(8);
        let step = self
            .target
            .dispatch_message(message, &mut CoreHost::new(&mut state), 100)?;
        Ok(u32::from(matches!(step, Some(Step::Event(_)))))
    }

    fn enqueue_message(&mut self, target: u32, message: [u32; 4]) -> Result<u32> {
        self.queued.push((target, message));
        Ok(1)
    }

    fn interpreter(&mut self, target: u32) -> Result<&mut Interpreter> {
        if target != 42 {
            return Err(Error::new(0, "任务不存在"));
        }
        Ok(&mut self.target)
    }
}

fn services(code: &[u8]) -> Services {
    Services {
        target: Interpreter::new(
            Program::from_code(code, vec![0; 64], 64).unwrap(),
            vec![0; 32],
        )
        .unwrap(),
        queued: vec![],
        dispatched: 0,
    }
}

#[test]
fn task_host_sends_messages_and_delegates_clock_and_identity_queries() {
    let mut receiver = Vec::new();
    ins(&mut receiver, 0x25, &[32]);
    ins(&mut receiver, 0x2b, &[71, 4]);
    ins(&mut receiver, 0x26, &[0]);
    ins(&mut receiver, 0x1e, &[36, 100]);
    ins(&mut receiver, 0x2d, &[]);
    ins(&mut receiver, 0x29, &[0]);
    let mut services = services(&receiver);
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    services.target.resume(&mut core, 100).unwrap();

    let mut sender = Vec::new();
    for (native, args, output) in [
        (68, vec![], 0),
        (5, vec![], 4),
        (69, vec![42, 9, 20, 30], 8),
        (70, vec![42, 10, 21, 31], 12),
        (69, vec![7, 11, 22, 32], 16),
        (70, vec![7, 12, 23, 33], 20),
    ] {
        for value in &args {
            ins(&mut sender, 0x25, &[*value]);
        }
        ins(&mut sender, 0x2b, &[native, args.len() as u32 * 4]);
        ins(&mut sender, 0x26, &[output]);
    }
    sender.push(0x3b);
    let mut task = OrdinaryTask::new(
        Program::from_code(&sender, vec![0; 64], 128).unwrap(),
        vec![],
    )
    .unwrap();
    task.task_id = 7;
    task.clock.origin = 10;
    let mut host = TaskHost::new(&mut core, &mut services);
    assert!(task.update(100, 1, &mut host, 100).unwrap());
    let values: Vec<_> = task
        .interpreter
        .read(0, 24)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(values, [7, 90, 1, 1, 0, 1]);
    assert_eq!(services.dispatched, 1);
    let received: Vec<_> = services
        .target
        .read(32, 16)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(received, [7, 109, 20, 30]);
    assert!(services.target.is_waiting_for_message());
    assert_eq!(
        services.queued,
        [(42, [7, 10, 21, 31]), (7, [7, 12, 23, 33])]
    );
}

fn args(values: [u32; 4]) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_le_bytes).collect()
}

#[test]
fn task_memory_reads_and_writes_other_vms_and_preserves_self_overlap() {
    let mut services = services(&[0x3b]);
    services.target.write(0, b"QRSTUVWXYZ").unwrap();
    let mut vm = Interpreter::new(
        Program::from_code(&[0x3b], vec![0; 64], 64).unwrap(),
        vec![0; 32],
    )
    .unwrap();
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut host = TaskHost::new(&mut core, &mut services);
    host.set_task_id(7);
    assert_eq!(
        host.call(72, &args([42, 2, 4, 0x8000_0001]), &mut vm)
            .unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(vm.read(0x8000_0001, 4).unwrap(), b"STUV");
    host.call(73, &args([42, 0x8000_0002, 4, 0x8000_0001]), &mut vm)
        .unwrap();
    assert_eq!(host.services.target.read(0x8000_0002, 4).unwrap(), b"STUV");
    vm.write(0, &(0..16).collect::<Vec<u8>>()).unwrap();
    host.memory_copy_mode = MemoryCopyMode::Dword;
    host.call(73, &args([7, 1, 8, 0]), &mut vm).unwrap();
    assert_eq!(
        vm.read(0, 16).unwrap(),
        [0, 0, 1, 2, 3, 3, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15]
    );
    assert!(host.call(72, &args([999, 0, 4, 0]), &mut vm).is_err());
    assert!(
        host.call(72, &args([42, 0, 4, 0x8000_001f]), &mut vm)
            .is_err()
    );
    assert!(host.call(70, &[], &mut vm).is_err());
}
