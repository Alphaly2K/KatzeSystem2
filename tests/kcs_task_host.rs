use katze_system2::{
    Error, Result, Step,
    expression::State,
    kcs::{
        CoreHost, Host, Interpreter, MemoryCopyMode, NativeReturn, OrdinaryTask, Program, TaskHost,
        TaskServices,
    },
    task::{Command, TaskTree},
};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

struct Services {
    tree: TaskTree,
    target: Interpreter,
    queued: Vec<(u32, [u32; 4])>,
    dispatched: usize,
    created: Vec<Creation>,
    deleted: Vec<(u32, u32)>,
}

#[derive(Debug, PartialEq)]
struct Creation {
    sender: u32,
    parent: u32,
    kind: u32,
    name: Vec<u8>,
    context: Vec<u8>,
}

impl TaskServices for Services {
    fn task_tree(&self) -> Option<&TaskTree> {
        Some(&self.tree)
    }

    fn create_task(
        &mut self,
        sender: u32,
        parent: u32,
        kind: u32,
        name: &[u8],
        context: &[u8],
    ) -> Result<u32> {
        let id = self.tree.insert_auto(parent, kind)?;
        self.created.push(Creation {
            sender,
            parent,
            kind,
            name: name.to_vec(),
            context: context.to_vec(),
        });
        Ok(id)
    }

    fn request_delete(&mut self, sender: u32, target: u32) -> Result<()> {
        self.deleted.push((sender, target));
        Ok(())
    }

    fn dispatch_message(&mut self, target: u32, message: &mut [u32; 4]) -> Result<u32> {
        let mut output = 0;
        let command = Command {
            target,
            sender: message[0],
            code: 173,
            data: [0, 0, 1],
        };
        let found =
            self.tree
                .dispatch(message[0], command, &mut output, |_, command, output| {
                    if command.target != 42 {
                        return Err(Error::new(0, "任务不提供接收 VM"));
                    }
                    self.dispatched += 1;
                    let mut state = State::new(8);
                    let step = self.target.dispatch_message(
                        message,
                        &mut CoreHost::new(&mut state),
                        100,
                    )?;
                    *output = u32::from(matches!(step, Some(Step::Event(_))));
                    Ok(false)
                })?;
        if !found {
            return Err(Error::new(0, "任务不存在"));
        }
        Ok(output)
    }

    fn enqueue_message(&mut self, target: u32, message: [u32; 4]) -> Result<u32> {
        let mut output = 0;
        let command = Command {
            target,
            sender: message[0],
            code: 174,
            data: [0, 0, 1],
        };
        let found =
            self.tree
                .dispatch(message[0], command, &mut output, |_, command, output| {
                    self.queued.push((command.target, message));
                    *output = 1;
                    Ok(false)
                })?;
        if !found {
            return Err(Error::new(0, "任务不存在"));
        }
        Ok(output)
    }

    fn interpreter(&mut self, target: u32) -> Result<&mut Interpreter> {
        if target != 42 {
            return Err(Error::new(0, "任务不存在"));
        }
        Ok(&mut self.target)
    }
}

fn services(code: &[u8]) -> Services {
    let mut tree = TaskTree::new(7, 12);
    tree.insert(7, 42, 12).unwrap();
    Services {
        tree,
        target: Interpreter::new(
            Program::from_code(code, vec![0; 64], 64).unwrap(),
            vec![0; 32],
        )
        .unwrap(),
        queued: vec![],
        dispatched: 0,
        created: vec![],
        deleted: vec![],
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

#[test]
fn creation_formats_names_and_passes_raw_context_before_requesting_deletion() {
    let mut services = services(&[0x3b]);
    let mut state = State::new(8);
    let mut core = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[], vec![0; 128], 32).unwrap(), vec![]).unwrap();
    vm.write(0, &17u32.to_le_bytes()).unwrap();
    vm.write(64, b"unit%d[G0]\0").unwrap();
    let mut host = TaskHost::new(&mut core, &mut services);
    host.set_task_id(7);
    host.creation_context = b"raw%d[G0]\0ignored".to_vec();
    let arguments: Vec<_> = [12u32, 42, 64]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(
        host.call(66, &arguments, &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(
        host.call(67, &0u32.to_le_bytes(), &mut vm).unwrap(),
        NativeReturn::Continue
    );
    assert_eq!(
        host.call(75, &0u32.to_le_bytes(), &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    host.set_task_id(0);
    assert_eq!(
        host.call(76, &[], &mut vm).unwrap(),
        NativeReturn::Value(42)
    );
    assert_eq!(
        services.created,
        [Creation {
            sender: 7,
            parent: 42,
            kind: 12,
            name: b"unit17".to_vec(),
            context: b"raw%d[G0]".to_vec()
        }]
    );
    assert_eq!(services.deleted, [(7, 0)]);
}
