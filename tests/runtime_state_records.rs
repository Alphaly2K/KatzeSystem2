use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, HostState, Interpreter, NativeReturn, Program, SharedMemory},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, TaskObject, kind},
    save::{StateNode, StateTree},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    code: u32,
    data: [u32; 3],
    raw: Option<Vec<u8>>,
    tree: Option<StateTree>,
}
#[derive(Clone)]
struct Case {
    index: u32,
    container: bool,
    invalid: bool,
    found: bool,
    available: bool,
    missing_view: bool,
    reply: u32,
    mutate: bool,
    root: u32,
    target: u32,
    tree: StateTree,
    record: Vec<u8>,
}
fn fixture() -> StateTree {
    StateTree {
        roots: vec![
            StateNode {
                data: vec![1, 2, 3],
                children: vec![
                    StateNode {
                        data: vec![4],
                        children: vec![],
                    },
                    StateNode {
                        data: vec![5, 6],
                        children: vec![],
                    },
                ],
            },
            StateNode {
                data: vec![7],
                children: vec![],
            },
        ],
    }
}
impl Default for Case {
    fn default() -> Self {
        Self {
            index: 689,
            container: true,
            invalid: false,
            found: true,
            available: true,
            missing_view: false,
            reply: 0,
            mutate: false,
            root: 0,
            target: 0,
            tree: fixture(),
            record: fixture().to_record().unwrap(),
        }
    }
}
struct Receiver {
    base: BaseObject,
    events: Rc<RefCell<Vec<Event>>>,
    case: Case,
    view: Option<SharedMemory>,
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
        if matches!(command.code, 437 | 445 | 446 | 447) {
            let mut event = Event {
                code: command.code,
                data: command.data,
                raw: None,
                tree: None,
            };
            if command.code == 437 {
                let bytes = buffers.data1.as_deref_mut().unwrap();
                event.raw = Some(bytes.to_vec());
                if self.case.mutate && !bytes.is_empty() {
                    bytes[0] ^= 0x55;
                }
            } else if command.code == 445 {
                if self.case.available {
                    buffers.external = self.view.clone();
                }
            } else if command.code == 446 {
                let view = buffers.external.as_ref().unwrap();
                event.tree = Some(StateTree::from_record(&view.read(0, view.len())?)?);
                if self.case.mutate && !view.is_empty() {
                    view.write(0, &[0x55])?;
                }
            } else {
                assert_eq!(command.data[1], 0x13579bdf);
                let view = buffers.external.as_ref().unwrap();
                if !view.is_empty() {
                    let old = view.read(0, 1)?[0];
                    view.write(0, &[old ^ 0x55])?;
                    assert_eq!(self.view.as_ref().unwrap().read(0, 1)?[0], old ^ 0x55);
                    view.write(0, &[old])?;
                }
                event.data[1] = 0;
                event.tree = Some(self.case.tree.clone());
            }
            self.events.borrow_mut().push(event);
            *output = if command.code == 445 {
                if self.case.available { 0x13579bdf } else { 0 }
            } else {
                self.case.reply
            };
            return Ok(CommandOutcome::finished(true));
        }
        self.base
            .dispatch_command_with_buffers(kind, command, buffers, output, host, budget)
    }
}

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn ins(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
    code.push(opcode);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn core_call(host: &mut CoreHost<'_>, vm: &mut Interpreter, index: u32, args: &[u32]) -> u32 {
    let args: Vec<_> = args.iter().flat_map(|n| n.to_le_bytes()).collect();
    match host.call(index, &args, vm).unwrap() {
        NativeReturn::Value(n) => n,
        _ => panic!(),
    }
}
#[derive(Debug)]
struct Outcome {
    value: u32,
    events: Vec<Event>,
    record: Option<Vec<u8>>,
    stored: Vec<u8>,
}
fn run(case: Case, budget: usize) -> Result<Outcome> {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0xabcdef01]);
    code.extend([0x25, 0x13]);
    code.extend(0u32.to_le_bytes());
    if case.container {
        for value in [7, case.target] {
            ins(&mut code, 0x25, &[value]);
        }
    }
    ins(
        &mut code,
        0x2b,
        &[case.index, if case.container { 12 } else { 4 }],
    );
    ins(&mut code, 0x26, &[4]);
    code.push(0x3b);
    let events = Rc::new(RefCell::new(Vec::new()));
    let provider = Receiver {
        base: BaseObject::new(),
        events: events.clone(),
        case: case.clone(),
        view: if case.missing_view {
            None
        } else {
            Some(SharedMemory::new(case.tree.to_record()?)?)
        },
    };
    let mut rt = Runtime::new(
        case.root,
        kind::ROOT,
        Box::new(provider),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(&code, vec![0; 64], 64)?)),
    );
    rt.set_budget(budget, 1 << 20);
    let mut numbers = State::new(0);
    let mut host = CoreHost {
        state: &mut numbers,
        host: HostState::with_handles(rt.host().handle_allocator()),
        task_tree: None,
    };
    let mut seed = Interpreter::new(Program::from_code(&[], case.record.clone(), 64)?, vec![])?;
    let container = core_call(&mut host, &mut seed, 238, &[0]);
    if case.found {
        core_call(
            &mut host,
            &mut seed,
            244,
            &[container, 7, case.record.len() as u32, 0],
        );
        core_call(
            &mut host,
            &mut seed,
            244,
            &[container, 7, case.record.len() as u32, 0],
        );
    }
    *rt.host_mut() = host.host;
    let task = rt.create(case.root, kind::ORDINARY, b"state", b"")?;
    let argument = if !case.container {
        0
    } else if case.invalid {
        u32::MAX
    } else {
        container
    };
    rt.object_mut(task)
        .unwrap()
        .interpreter_mut()
        .unwrap()
        .write(0, &argument.to_le_bytes())?;
    rt.update(0, 0)?;
    let vm = rt.object(task).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), 0xabcdef01u32.to_le_bytes());
    let value = u32::from_le_bytes(vm.read(4, 4)?.as_ref().try_into().unwrap());
    let record = rt
        .host()
        .data_container(container)?
        .record(7)?
        .map(<[u8]>::to_vec);
    let events = events.borrow().clone();
    let stored = rt.host().data_container(container)?.used_bytes()?.to_vec();
    Ok(Outcome {
        value,
        events,
        record,
        stored,
    })
}

#[test]
fn state_record_natives_keep_query_release_restore_results_and_record_aliases() {
    for budget in [1, 4096] {
        for index in [576, 688, 689] {
            for container in [false, true] {
                for found in [false, true] {
                    for available in [false, true] {
                        for reply in [0, 0xfedcba98] {
                            let case = Case {
                                index,
                                container,
                                found,
                                available,
                                reply,
                                mutate: index == 576,
                                ..Default::default()
                            };
                            let result = run(case.clone(), budget).unwrap();
                            let expected = container
                                && match index {
                                    576 => found && reply != 0,
                                    688 => available,
                                    _ => found,
                                };
                            assert_eq!(result.value, u32::from(expected));
                            let codes: Vec<_> = result.events.iter().map(|e| e.code).collect();
                            let expected_codes: Vec<_> = if !container {
                                vec![]
                            } else {
                                match index {
                                    576 if found => vec![437],
                                    688 if available => vec![445, 447],
                                    688 => vec![445],
                                    689 if found => vec![446],
                                    _ => vec![],
                                }
                            };
                            assert_eq!(codes, expected_codes);
                            if index == 576 && container && found {
                                assert_eq!(result.events[0].raw.as_ref().unwrap(), &case.record);
                                let mut changed = case.record;
                                assert_eq!(&result.stored[16 + changed.len()..], changed);
                                changed[0] ^= 0x55;
                                assert_eq!(result.record, Some(changed));
                            } else if index == 688 && container && available && !found {
                                assert_eq!(result.record, Some(case.tree.to_record().unwrap()));
                            } else {
                                assert_eq!(result.record, case.found.then_some(case.record));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn state_record_natives_distinguish_empty_trees_missing_targets_and_invalid_records() {
    for budget in [1, 4096] {
        let result = run(
            Case {
                index: 688,
                tree: StateTree::default(),
                invalid: true,
                ..Default::default()
            },
            budget,
        )
        .unwrap();
        assert_eq!(result.value, 0);
        assert_eq!(
            result.events.iter().map(|e| e.code).collect::<Vec<_>>(),
            [445, 447]
        );
        let tree = StateTree {
            roots: vec![StateNode {
                data: vec![99],
                children: vec![],
            }],
        };
        let result = run(
            Case {
                index: 688,
                tree: tree.clone(),
                ..Default::default()
            },
            budget,
        )
        .unwrap();
        assert_eq!(result.record, Some(fixture().to_record().unwrap()));
        assert!(result.stored.ends_with(&tree.to_record().unwrap()));
        for index in [576, 688, 689] {
            let result = run(
                Case {
                    index,
                    target: 7,
                    ..Default::default()
                },
                budget,
            )
            .unwrap();
            assert_eq!(result.value, u32::from(index == 689));
            assert!(result.events.is_empty());
        }
        let result = run(
            Case {
                index: 689,
                mutate: true,
                ..Default::default()
            },
            budget,
        )
        .unwrap();
        assert_eq!(result.record, Some(fixture().to_record().unwrap()));
        assert_eq!(
            result.events[0].tree,
            Some(StateTree::from_record(&fixture().to_record().unwrap()).unwrap())
        );
        assert!(
            run(
                Case {
                    index: 689,
                    target: 7,
                    record: vec![0; 11],
                    ..Default::default()
                },
                budget
            )
            .is_err()
        );
        assert!(
            run(
                Case {
                    index: 688,
                    missing_view: true,
                    ..Default::default()
                },
                budget
            )
            .unwrap_err()
            .message
            .contains("编码视图")
        );
        let result = run(
            Case {
                index: 688,
                available: false,
                invalid: true,
                ..Default::default()
            },
            budget,
        )
        .unwrap();
        assert_eq!(result.value, 0);
        assert!(
            run(
                Case {
                    index: 688,
                    invalid: true,
                    ..Default::default()
                },
                budget
            )
            .is_err()
        );
    }
}
