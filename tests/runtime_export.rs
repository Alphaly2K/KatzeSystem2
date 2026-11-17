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
        if matches!(command.code, 453 | 454 | 516 | 517 | 583..=588) {
            let name = |bytes: &[u8]| bytes[..bytes.iter().position(|&b| b == 0).unwrap()].to_vec();
            self.events.borrow_mut().push(Event {
                command: *command,
                first: if matches!(command.code, 453 | 583) {
                    buffers.data1.as_deref().unwrap().to_vec()
                } else if matches!(command.code, 516 | 584..=586 | 588) {
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
    runtime.host_mut().folder_paths = Some(katze_system2::resource::FolderPaths {
        user_data: "USER%title%".encode_utf16().collect(),
        common_data: "COMMON".encode_utf16().collect(),
        brand: "BRAND%title%".encode_utf16().collect(),
        title: "TITLE".encode_utf16().collect(),
        save: "SAVE/資料/🌟".encode_utf16().collect(),
        imglib: "IMAGES".encode_utf16().collect(),
    });
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

#[test]
fn network_configuration_routes_two_byte_fields_and_a_byte_expanded_folder() -> Result<()> {
    for (path, wide) in [
        (Vec::new(), Vec::new()),
        (vec![0x82, 0xa0], vec![0xff82, 0xffa0]),
        (
            b"%title%\x82\xa0".to_vec(),
            [
                "TITLE".encode_utf16().collect::<Vec<_>>(),
                vec![0xff82, 0xffa0],
            ]
            .concat(),
        ),
        (
            b"%save%/%USERPROFILE%/%brand%/%imglib%".to_vec(),
            "SAVE/資料/🌟/USER%title%/BRAND%title%/IMAGES"
                .encode_utf16()
                .collect(),
        ),
        (b"%\x82\xa0%".to_vec(), "%あ%".encode_utf16().collect()),
        (vec![b'x'; 511], vec![u16::from(b'x'); 511]),
    ] {
        let mut data = vec![0; 4096];
        data[16..20].copy_from_slice(&7u32.to_le_bytes());
        let first = b"user%u[G16]";
        data[64..64 + first.len()].copy_from_slice(first);
        let second = b"key%c[G24]ignored";
        data[600..600 + second.len()].copy_from_slice(second);
        data[1200..1200 + path.len()].copy_from_slice(&path);
        let mut record = vec![0; 2048];
        record[..5].copy_from_slice(b"user7");
        record[512..515].copy_from_slice(b"key");
        for (bytes, unit) in record[1024..].chunks_exact_mut(2).zip(wide) {
            bytes.copy_from_slice(&unit.to_le_bytes());
        }
        for external in [0, 0x8000_0000] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0x1357_9bdf]);
            for target in [7, u32::MAX, 0] {
                for value in [target, external | 64, external | 600, external | 1200] {
                    instruction(&mut code, 0x25, &[value]);
                }
                instruction(&mut code, 0x2b, &[1018, 16]);
                instruction(&mut code, 0x25, &[target]);
                instruction(&mut code, 0x2b, &[601, 4]);
                instruction(&mut code, 0x26, &[if target == u32::MAX { 4 } else { 0 }]);
                instruction(&mut code, 0x25, &[target]);
                instruction(&mut code, 0x2b, &[1022, 4]);
                instruction(&mut code, 0x26, &[if target == u32::MAX { 4 } else { 8 }]);
            }
            code.push(0x3b);
            for budget in [1, 4096] {
                let (events, resolutions) = run(&code, &data, budget, 0xabcdef01)?;
                assert!(resolutions.is_empty());
                let expected: Vec<_> = [7, 0]
                    .into_iter()
                    .flat_map(|target| {
                        [583, 454, 587]
                            .into_iter()
                            .map(|code| Event {
                                command: Command {
                                    sender: 8,
                                    target,
                                    code,
                                    data: [0; 3],
                                },
                                first: if code == 583 {
                                    record.clone()
                                } else {
                                    Vec::new()
                                },
                                second: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                assert_eq!(events, expected);
            }
        }
    }
    for field in 0..3 {
        let mut data = vec![0; 4096];
        let start = [64, 600, 1200][field];
        data[start..start + 512].fill(b'x');
        let mut code = Vec::new();
        for value in [7, 64, 600, 1200] {
            instruction(&mut code, 0x25, &[value]);
        }
        instruction(&mut code, 0x2b, &[1018, 16]);
        code.push(0x3b);
        assert!(
            run(&code, &data, 4096, 0)
                .unwrap_err()
                .to_string()
                .contains("字段容量")
        );
    }
    Ok(())
}

#[test]
fn network_paths_use_wide_expansion_and_start_requests_read_retained_argument_slots() -> Result<()>
{
    for (path, expected) in [
        (b"".to_vec(), b"".to_vec()),
        (vec![0x82, 0xa0], vec![0x82, 0xa0]),
        (vec![0x81, 0x7f], vec![0x81, 0x45]),
        (vec![0x82], vec![0x81, 0x45]),
        (
            vec![0x80, 0xa0, 0xfd, 0xfe, 0xff, 0xf0, 0x40],
            vec![0x80, 0xa0, 0xfd, 0xfe, 0xff, 0xf0, 0x40],
        ),
        (
            b"%save%/image.png".to_vec(),
            b"SAVE/\x8e\x91\x97\xbf/??/image.png".to_vec(),
        ),
        (
            b"before%u[G16]/%%title%%/%%brand%%".to_vec(),
            b"before7/TITLE/BRAND%title%".to_vec(),
        ),
        (
            b"%userprofile%/%title".to_vec(),
            b"%userprofile%/%title".to_vec(),
        ),
        (vec![b'x'; 2047], vec![b'x'; 2047]),
    ] {
        let mut data = vec![0; 4096];
        data[16..20].copy_from_slice(&7u32.to_le_bytes());
        data[64..64 + path.len()].copy_from_slice(&path);
        data[3000..3026].copy_from_slice(b"caption%u[G16]%c[G24]tail\0");
        for external in [0, 0x8000_0000] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0x1357_9bdf]);
            for target in [7, u32::MAX, 0] {
                for (native, arity) in [(1019, 8), (1021, 8), (1021, 4)] {
                    instruction(&mut code, 0x25, &[target]);
                    instruction(
                        &mut code,
                        0x25,
                        &[external | if native == 1019 { 64 } else { 3000 }],
                    );
                    if arity == 4 {
                        code.push(0x2d);
                    }
                    instruction(&mut code, 0x2b, &[native, arity]);
                }
                for (native, output) in [(601, 0), (1022, 8)] {
                    instruction(&mut code, 0x25, &[target]);
                    instruction(&mut code, 0x2b, &[native, 4]);
                    instruction(
                        &mut code,
                        0x26,
                        &[if target == u32::MAX { 4 } else { output }],
                    );
                }
            }
            code.push(0x3b);
            for budget in [1, 4096] {
                let (events, resolutions) = run(&code, &data, budget, 0xabcdef01)?;
                assert!(resolutions.is_empty());
                let expected: Vec<_> = [7, 0]
                    .into_iter()
                    .flat_map(|target| {
                        [584, 586, 586, 454, 587]
                            .into_iter()
                            .map(|code| Event {
                                command: Command {
                                    sender: 8,
                                    target,
                                    code,
                                    data: [0; 3],
                                },
                                first: match code {
                                    584 => expected.clone(),
                                    586 => b"caption7".to_vec(),
                                    _ => Vec::new(),
                                },
                                second: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                assert_eq!(events, expected);
            }
        }
    }
    let mut code = Vec::new();
    for value in [7, u32::MAX] {
        instruction(&mut code, 0x25, &[value]);
    }
    code.push(0x2d);
    instruction(&mut code, 0x2b, &[1021, 4]);
    code.push(0x3b);
    assert!(run(&code, &[0; 64], 1, 0).is_err());
    for (path, diagnostic) in [
        (vec![b'x'; 2048], "转换缓冲区"),
        ([0x82, 0xa0].repeat(1024), "字节缓冲区"),
        (b"%brand%".repeat(292), "字段容量"),
    ] {
        let mut data = vec![0; 4096];
        data[64..64 + path.len()].copy_from_slice(&path);
        let mut code = Vec::new();
        for value in [7, 64] {
            instruction(&mut code, 0x25, &[value]);
        }
        instruction(&mut code, 0x2b, &[1019, 8]);
        code.push(0x3b);
        assert!(
            run(&code, &data, 1, 0)
                .unwrap_err()
                .message
                .contains(diagnostic)
        );
    }
    Ok(())
}
