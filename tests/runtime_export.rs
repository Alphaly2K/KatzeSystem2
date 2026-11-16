use katze_system2::{
    Result,
    expression::State,
    kcs::{Host, Interpreter, Program, SharedMemory},
    runtime::{BaseObject, KcsFactory, ObjectFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    command: Command,
    first: Vec<u8>,
    second: Option<Vec<u8>>,
}

struct Receiver {
    base: BaseObject,
    events: Rc<RefCell<Vec<Event>>>,
    reply: u32,
}

impl TaskObject for Receiver {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.base.update_body(host, increment, budget)
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if matches!(command.code, 453 | 454 | 516 | 517 | 585 | 587 | 588) {
            let name = |bytes: &[u8]| bytes[..bytes.iter().position(|&b| b == 0).unwrap()].to_vec();
            self.events.borrow_mut().push(Event {
                command: *command,
                first: if command.code == 453 {
                    buffers.data1.as_deref().unwrap().to_vec()
                } else if matches!(command.code, 516 | 585 | 588) {
                    name(buffers.data1.as_deref().unwrap())
                } else {
                    Vec::new()
                },
                second: buffers.data0.as_deref().map(name),
            });
            if let Some(text) = buffers.data1.as_deref_mut() {
                text[0] = b'Z';
            }
            if let Some(text) = buffers.data0.as_deref_mut() {
                text[0] = b'Z';
            }
            *output = self.reply;
            return Ok(CommandOutcome::finished(true));
        }
        self.base
            .dispatch_command_with_buffers(kind, command, buffers, output, host, budget)
    }
}

struct Programs(Program);
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

struct Factory {
    kcs: KcsFactory<Programs>,
    events: Rc<RefCell<Vec<Event>>>,
    reply: u32,
}
impl ObjectFactory for Factory {
    fn external_storage(&self) -> SharedMemory {
        self.kcs.external_storage()
    }
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::BASE {
            return Ok(Box::new(Receiver {
                base: BaseObject::new(),
                events: self.events.clone(),
                reply: self.reply,
            }));
        }
        self.kcs.create(id, object_kind, name)
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Resolution(Vec<u8>, Vec<u8>);
struct Resources(Rc<RefCell<Vec<Resolution>>>);
impl katze_system2::kcs::ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.0
            .borrow_mut()
            .push(Resolution(category.to_vec(), name.to_vec()));
        let mut result = category.to_vec();
        result.push(b'/');
        result.extend(name);
        result.extend(b"\0ignored");
        Ok(result)
    }
    fn read(
        &mut self,
        _: &[u8],
        _: katze_system2::resource::ResourceMode,
    ) -> Result<Option<Vec<u8>>> {
        unreachable!()
    }
}

fn run(
    code: &[u8],
    data: &[u8],
    budget: usize,
    reply: u32,
) -> Result<(Vec<Event>, Vec<Resolution>)> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let resolutions = Rc::new(RefCell::new(Vec::new()));
    let factory = Factory {
        kcs: KcsFactory {
            programs: Programs(Program::from_code(code, data.to_vec(), 64)?),
            external: SharedMemory::new(data.to_vec())?,
        },
        events: events.clone(),
        reply,
    };
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(Receiver {
            base: BaseObject::new(),
            events: events.clone(),
            reply,
        }),
        State::new(0),
        factory,
    );
    runtime.set_budget(budget, 1 << 20);
    runtime.set_resources(Resources(resolutions.clone()));
    for _ in 0..7 {
        runtime.create(0, kind::BASE, b"receiver", b"")?;
    }
    let task = runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    assert_eq!(task, 8);
    runtime.update(0, 1)?;
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
    assert_eq!(
        vm.read(0, 12)?,
        [reply, 0, reply]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(vm.read(12, data.len() - 12)?, &data[12..]);
    assert_eq!(vm.read(0x8000_0000, data.len())?, data);
    let result = (events.borrow().clone(), resolutions.borrow().clone());
    Ok(result)
}

#[test]
fn export_natives_route_ordered_paths_and_keep_status_results_without_files() -> Result<()> {
    let mut data = vec![0; 1024];
    data[16..20].copy_from_slice(&7u32.to_le_bytes());
    data[64..79].copy_from_slice(b"first%u[G16]\0xx");
    data[512..536].copy_from_slice(b"second%c[G24]ignored\0xxx");
    for external in [0, 0x8000_0000] {
        let mut code = Vec::new();
        instruction(&mut code, 0x25, &[0x1357_9bdf]);
        for (slot, target) in [7, u32::MAX, 0].into_iter().enumerate() {
            for (index, arguments) in [
                (690, vec![target, external | 64]),
                (692, vec![target, external | 64, external | 512]),
                (691, vec![target]),
            ] {
                for &value in &arguments {
                    instruction(&mut code, 0x25, &[value]);
                }
                instruction(&mut code, 0x2b, &[index, arguments.len() as u32 * 4]);
                if index == 691 {
                    instruction(&mut code, 0x26, &[slot as u32 * 4]);
                }
            }
        }
        code.push(0x3b);
        for budget in [1, 4096] {
            for reply in [0, 1, 0xabcdef01] {
                let (events, resolutions) = run(&code, &data, budget, reply)?;
                let mut expected = Vec::new();
                for target in [7, 0] {
                    for (code, second) in [
                        (516, None),
                        (516, Some(b"export/second".to_vec())),
                        (517, None),
                    ] {
                        expected.push(Event {
                            command: Command {
                                target,
                                sender: 8,
                                code,
                                data: [0; 3],
                            },
                            first: if code == 516 {
                                b"export/first7".to_vec()
                            } else {
                                Vec::new()
                            },
                            second,
                        });
                    }
                }
                assert_eq!(events, expected);
                let expected: Vec<_> = (0..3)
                    .flat_map(|_| [b"first7".as_slice(), b"first7", b"second"])
                    .map(|name| Resolution(b"export".to_vec(), name.to_vec()))
                    .collect();
                assert_eq!(resolutions, expected);
            }
        }
    }
    Ok(())
}

#[test]
fn system_task_natives_route_wide_records_text_and_full_query_results() -> Result<()> {
    for (name, normalized) in [
        (b"art%u[G16]".as_slice(), b"art7".as_slice()),
        (
            &[0x80, 0xa0, 0xfd, 0xfe, 0xff],
            &[0x80, 0xa0, 0xfd, 0xfe, 0xff],
        ),
        (&[0x81, 0x7f], &[0x81, 0x45]),
    ] {
        let mut data = vec![0; 1024];
        data[16..20].copy_from_slice(&7u32.to_le_bytes());
        data[64..64 + name.len()].copy_from_slice(name);
        data[512..536].copy_from_slice(b"body%c[G24]ignored\0xxxxx");
        let mut record = vec![0; 780];
        let private = "export/\u{80}\u{f8f0}\u{f8f1}\u{f8f2}\u{f8f3}";
        let wide: Vec<_> = match normalized.first() {
            Some(0x80) => private.encode_utf16().collect(),
            Some(0x81) => "export/・".encode_utf16().collect(),
            _ => "export/art7".encode_utf16().collect(),
        };
        for (bytes, unit) in record.chunks_exact_mut(2).zip(wide) {
            bytes.copy_from_slice(&unit.to_le_bytes());
        }
        record[520..524].copy_from_slice(b"body");
        for external in [0, 0x8000_0000] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0x1357_9bdf]);
            for target in [7, u32::MAX, 0] {
                for (index, arguments, output) in [
                    (600, vec![target, external | 64, external | 512], None),
                    (1020, vec![target, external | 512], None),
                    (1023, vec![target, external | 512], None),
                    (
                        601,
                        vec![target],
                        Some(if target == u32::MAX { 4 } else { 0 }),
                    ),
                    (
                        1022,
                        vec![target],
                        Some(if target == u32::MAX { 4 } else { 8 }),
                    ),
                ] {
                    for &value in &arguments {
                        instruction(&mut code, 0x25, &[value]);
                    }
                    instruction(&mut code, 0x2b, &[index, arguments.len() as u32 * 4]);
                    if let Some(output) = output {
                        instruction(&mut code, 0x26, &[output]);
                    }
                }
            }
            code.push(0x3b);
            for budget in [1, 4096] {
                for reply in [0, 1, 0xabcdef01] {
                    let (events, resolutions) = run(&code, &data, budget, reply)?;
                    let expected: Vec<_> = [7, 0]
                        .into_iter()
                        .flat_map(|target| {
                            [453, 585, 588, 454, 587]
                                .into_iter()
                                .map(|code| Event {
                                    command: Command {
                                        sender: 8,
                                        target,
                                        code,
                                        data: if code == 453 { [2, 0, 0] } else { [0; 3] },
                                    },
                                    first: match code {
                                        453 => record.clone(),
                                        585 | 588 => b"body".to_vec(),
                                        _ => Vec::new(),
                                    },
                                    second: None,
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    assert_eq!(events, expected);
                    assert_eq!(
                        resolutions,
                        vec![Resolution(b"export".to_vec(), normalized.to_vec()); 3]
                    );
                }
            }
        }
    }
    for (name_length, text_length, expected) in [
        (253, 0, "资源名称超过字段容量"),
        (3, 260, "文字超过字段容量"),
    ] {
        let mut data = vec![0; 1024];
        data[64..64 + name_length].fill(b'a');
        data[512..512 + text_length].fill(b'b');
        let mut code = Vec::new();
        for value in [7, 64, 512] {
            instruction(&mut code, 0x25, &[value]);
        }
        instruction(&mut code, 0x2b, &[600, 12]);
        code.push(0x3b);
        assert!(
            run(&code, &data, 4096, 0)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
    Ok(())
}
