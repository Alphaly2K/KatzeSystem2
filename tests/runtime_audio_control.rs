use katze_system2::{
    Result,
    expression::State,
    kcs::{Host, Interpreter, Program},
    runtime::{BaseObject, KcsFactory, ObjectFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, rc::Rc};

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn arguments(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn program(index: u32, args: &[u8]) -> Program {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[0xdeadbeef]);
    let mut stack = vec![0xa5; (4 - args.len() % 4) % 4];
    stack.extend(args);
    for word in stack.chunks_exact(4) {
        instruction(
            &mut code,
            0x25,
            &[u32::from_le_bytes(word.try_into().unwrap())],
        );
    }
    instruction(&mut code, 0x2b, &[index, args.len() as u32]);
    instruction(&mut code, 0x3b, &[]);
    Program::from_code(&code, vec![0x5a; 1024], 128).unwrap()
}

struct Source(Program);
impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[derive(Debug, PartialEq)]
struct Event(Command, Vec<u8>);

struct Trace {
    events: Vec<Event>,
    output: u32,
    sound_target: u32,
    buffer_output: u32,
    write_buffer: bool,
}

struct Factory {
    scripts: KcsFactory<Source>,
    trace: Rc<RefCell<Trace>>,
}

impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::SOUND {
            Ok(Box::new(Receiver(BaseObject::new(), self.trace.clone())))
        } else {
            self.scripts.create(id, kind, name)
        }
    }
}

struct Receiver(BaseObject, Rc<RefCell<Trace>>);
impl TaskObject for Receiver {
    fn control(&self) -> &Control {
        self.0.control()
    }
    fn clock(&self) -> &TaskClock {
        self.0.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.0.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.0.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.0.update_body(host, increment, budget)
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
        if command.code != 35
            && !(182..=194).contains(&command.code)
            && !(196..=198).contains(&command.code)
        {
            return self.0.dispatch_command(kind, command, output, host, budget);
        }
        let mut trace = self.1.borrow_mut();
        trace.events.push(Event(
            *command,
            if command.code == 198 {
                vec![]
            } else {
                buffers.data1.as_deref().unwrap_or_default().to_vec()
            },
        ));
        *output = if command.code == 35 {
            trace.sound_target
        } else {
            trace.output
        };
        if let Some(buffer) = buffers.data1.as_deref_mut() {
            if command.code == 198 && trace.write_buffer {
                buffer[..4].copy_from_slice(&trace.buffer_output.to_le_bytes());
            } else if command.code != 198 {
                buffer.fill(0xff);
            }
        }
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.0.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.0.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.0.request_delete(host, budget)
    }
}

fn setup(program: Program, output: u32) -> (Runtime<Factory>, Rc<RefCell<Trace>>) {
    let trace = Rc::new(RefCell::new(Trace {
        events: vec![],
        output,
        sound_target: 1,
        buffer_output: 0x734512ab,
        write_buffer: true,
    }));
    let mut scripts = KcsFactory::new(Source(program));
    scripts.external = katze_system2::memory::SharedMemory::new(vec![0x6b; 512]).unwrap();
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(Receiver(BaseObject::new(), trace.clone())),
        State::new(0),
        Factory {
            scripts,
            trace: trace.clone(),
        },
    );
    assert_eq!(rt.create(0, kind::SOUND, b"sound", b"").unwrap(), 1);
    assert_eq!(rt.create(0, kind::ORDINARY, b"caller", b"").unwrap(), 2);
    (rt, trace)
}

#[test]
fn control_wrappers_route_words_and_raw_float_buffers_without_changing_sources() {
    let cases = [
        (156, vec![u32::MAX], 0, vec![]),
        (157, vec![0x80000000, u32::MAX], u32::MAX, vec![]),
        (158, vec![0], 0, vec![]),
        (159, vec![31], 0, vec![]),
        (160, vec![64], 0, vec![]),
        (161, vec![63, 0xffffffbb], 0xffffffbb, vec![]),
        (162, vec![32], 0, vec![]),
        (163, vec![7, 0xaaaa1234, 0xdead5678], 0x56781234, vec![]),
        (164, vec![65536, 0x80000000], 0x80000000, vec![]),
        (165, vec![u32::MAX], 0, vec![]),
        (166, vec![9, 0x1111ffff, 0x22228000], 0x8000ffff, vec![]),
        (
            167,
            vec![1, 0x7fa12345, 0x80000000],
            0,
            vec![0x7fa12345, 0x80000000],
        ),
        (
            168,
            vec![2, 0x7f800000, 0xffa23456, 0x80000000],
            0,
            vec![0x7f800000, 0xffa23456, 0x80000000],
        ),
        (
            347,
            vec![5, 0xffa23456, 0x80000000, 0x7f800000],
            0,
            vec![0xffa23456, 0x80000000, 0x7f800000],
        ),
        (348, vec![5, 0xffa23456], 0, vec![0xffa23456]),
        (349, vec![5], 0, vec![]),
    ];
    for budget in [1, 4096] {
        for (index, words, data1, buffer) in &cases {
            for extra in [false, true] {
                let mut args = arguments(words);
                if matches!(index, 163 | 166) {
                    args.truncate(10);
                }
                if extra {
                    args.extend(arguments(&[0xabcdef12]));
                }
                let (mut rt, trace) = setup(program(*index, &args), 0x81234567);
                trace.borrow_mut().events.clear();
                rt.set_budget(budget, 4096);
                rt.update(0, 0).unwrap();
                assert_eq!(
                    trace.borrow().events,
                    [
                        Event(
                            Command {
                                target: 0,
                                sender: 2,
                                code: 35,
                                data: [0; 3]
                            },
                            vec![]
                        ),
                        Event(
                            Command {
                                target: 1,
                                sender: 2,
                                code: if *index >= 347 {
                                    196 + index - 347
                                } else {
                                    182 + index - 156
                                },
                                data: [words[0], *data1, 0]
                            },
                            arguments(buffer)
                        ),
                    ]
                );
                let vm = rt.object(2).unwrap().interpreter().unwrap();
                let mut expected = 0xdeadbeefu32.to_le_bytes().to_vec();
                expected.extend(vec![0xa5; (4 - args.len() % 4) % 4]);
                if matches!(index, 160 | 162 | 165) {
                    expected.extend(0x81234567u32.to_le_bytes());
                } else if *index == 349 {
                    expected.extend(0x734512abu32.to_le_bytes());
                }
                assert_eq!(vm.stack_bytes(), expected, "native {index}");
                assert_eq!(vm.read(0, 1024).unwrap(), vec![0x5a; 1024]);
                assert_eq!(vm.read(0x80000000, 512).unwrap(), vec![0x6b; 512]);
            }
        }
    }
}

#[test]
fn invalid_queries_and_short_arguments_diagnose_after_the_root_lookup() {
    for (index, length) in [
        (156, 4),
        (157, 8),
        (163, 10),
        (166, 10),
        (167, 12),
        (168, 16),
        (347, 16),
        (348, 8),
        (349, 4),
    ] {
        let (mut rt, trace) = setup(program(index, &vec![0; length - 1]), 0);
        trace.borrow_mut().events.clear();
        assert!(rt.update(0, 0).unwrap_err().message.contains("范围"));
        assert_eq!(trace.borrow().events.len(), 1);
        assert_eq!(trace.borrow().events[0].0.code, 35);
    }
    for index in [160, 162, 165, 349] {
        for target in [2, u32::MAX] {
            let (mut rt, trace) = setup(program(index, &arguments(&[0])), 0);
            trace.borrow_mut().events.clear();
            trace.borrow_mut().sound_target = target;
            assert!(
                rt.update(0, 0)
                    .unwrap_err()
                    .message
                    .contains("声音查询目标")
            );
            assert_eq!(trace.borrow().events.len(), 1);
        }
    }
    let (mut rt, trace) = setup(program(349, &arguments(&[0])), 0);
    trace.borrow_mut().write_buffer = false;
    assert!(rt.update(0, 0).unwrap_err().message.contains("结果缓冲区"));
    let (mut rt, trace) = setup(program(161, &arguments(&[0, 100])), 0);
    trace.borrow_mut().sound_target = u32::MAX;
    rt.update(0, 0).unwrap();
    assert_eq!(
        rt.object(2).unwrap().interpreter().unwrap().stack_bytes(),
        0xdeadbeefu32.to_le_bytes()
    );
}
