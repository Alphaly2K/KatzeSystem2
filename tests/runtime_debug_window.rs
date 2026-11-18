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
    text: Vec<u16>,
    record: Vec<u8>,
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
        if matches!(command.code, 328..=335 | 518 | 519 | 524) {
            let text = if command.code == 331 {
                buffers
                    .data1
                    .as_deref()
                    .unwrap()
                    .chunks_exact(2)
                    .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
                    .take_while(|&unit| unit != 0)
                    .collect()
            } else {
                Vec::new()
            };
            self.events.borrow_mut().push(Event {
                command: *command,
                text,
                record: if command.code == 330 {
                    buffers.data1.as_deref().unwrap().to_vec()
                } else {
                    Vec::new()
                },
            });
            if let Some(text) = buffers.data1.as_deref_mut() {
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

fn run(code: &[u8], data: &[u8], budget: usize, reply: u32) -> Result<Vec<Event>> {
    let events = Rc::new(RefCell::new(Vec::new()));
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
    for _ in 0..7 {
        runtime.create(0, kind::BASE, b"receiver", b"")?;
    }
    let task = runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    assert_eq!(task, 8);
    runtime.update(0, 1)?;
    let vm = runtime.object(task).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
    assert_eq!(vm.read(0, data.len())?, data);
    assert_eq!(vm.read(0x8000_0000, data.len())?, data);
    let result = events.borrow().clone();
    Ok(result)
}

#[test]
fn developer_natives_route_raw_values_and_formatted_utf16_without_a_window() -> Result<()> {
    let mut data = vec![0; 512];
    data[16..20].copy_from_slice(&7u32.to_le_bytes());
    let text = "A🐈\\n:%u[G16]\0".as_bytes();
    data[64..64 + text.len()].copy_from_slice(text);
    for address in [64, 0x8000_0040] {
        let mut code = Vec::new();
        instruction(&mut code, 0x25, &[0x1357_9bdf]);
        for (index, target, value) in [
            (279, 7, 0),
            (280, 7, u32::MAX),
            (282, 7, address),
            (283, 7, 0),
            (287, 7, 0),
            (288, 7, 0x8765_4321),
            (279, u32::MAX, 0),
            (282, u32::MAX, address),
            (279, 0, 0),
            (281, 7, address),
            (281, 7, u32::MAX),
        ] {
            instruction(&mut code, 0x25, &[target]);
            let count = if matches!(index, 280..=282 | 288) {
                instruction(&mut code, 0x25, &[value]);
                8
            } else {
                4
            };
            instruction(&mut code, 0x2b, &[index, count]);
        }
        code.push(0x3b);
        for budget in [1, 4096] {
            let events = run(&code, &data, budget, 0xabcdef01)?;
            let mut expected: Vec<_> = [
                (328, 0),
                (329, u32::MAX),
                (331, 0),
                (332, 0),
                (334, 0),
                (335, 0x8765_4321),
                (328, 0),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (code, value))| Event {
                command: Command {
                    sender: 8,
                    target: if i == 6 { 0 } else { 7 },
                    code,
                    data: [0, value, 0],
                },
                text: if code == 331 {
                    vec![65, 0xd83d, 92, 110, 58, 55]
                } else {
                    vec![]
                },
                record: Vec::new(),
            })
            .collect();
            expected.extend([
                Event {
                    command: Command {
                        sender: 8,
                        target: 7,
                        code: 330,
                        data: [0; 3],
                    },
                    text: Vec::new(),
                    record: data[64..416].to_vec(),
                },
                Event {
                    command: Command {
                        sender: 8,
                        target: 7,
                        code: 333,
                        data: [0, 4, 0],
                    },
                    text: Vec::new(),
                    record: Vec::new(),
                },
            ]);
            assert_eq!(events, expected);
        }
    }
    Ok(())
}

#[test]
fn legacy_natives_preserve_task_targets_raw_values_and_stack() -> Result<()> {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x1357_9bdf]);
    for (index, args) in [
        (837, &[0][..]),
        (837, &[0, u32::MAX]),
        (837, &[7, 0x8000_0000]),
        (838, &[0, u32::MAX]),
        (838, &[u32::MAX, 1]),
        (843, &[7]),
    ] {
        for arg in args {
            instruction(&mut code, 0x25, &[*arg]);
        }
        instruction(&mut code, 0x2b, &[index, args.len() as u32 * 4]);
    }
    code.push(0x3b);
    let events = run(&code, &[0; 8], 1, 0xf135_79bd)?;
    let commands: Vec<_> = events.into_iter().map(|event| event.command).collect();
    assert_eq!(
        commands,
        [(7, 518, 0x8000_0000), (0, 519, u32::MAX), (7, 524, 0)].map(|(target, code, value)| {
            Command {
                target,
                sender: 8,
                code,
                data: [0, value, 0],
            }
        },),
    );
    Ok(())
}
