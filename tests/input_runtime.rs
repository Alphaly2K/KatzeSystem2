use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use katze_system2::{
    Error, Result, expression,
    host::input::{InputDevice, KeySample, PointerSample},
    input::{HitMask, InputFactory, KeyEvent, KeyEventKind, PointerMotion},
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};

#[derive(Debug)]
struct Record {
    packet: Command,
    selected: u32,
    raw: u32,
    points: Vec<u8>,
}

type Records = Rc<RefCell<Vec<Record>>>;

#[test]
fn ordinary_image_cursor_tracks_hotspot_visibility_and_replacement() -> Result<()> {
    use katze_system2::{
        graphics::{Projection, RenderTarget, SoftwareBackend, TextureFilter, Transform},
        kcs::ResourceServices,
        resource::ResourceMode,
        runtime::{KcsFactory, ProgramLoader, RootObject},
    };

    struct Source(Vec<u8>);
    impl ProgramLoader for Source {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut data = vec![0; 128];
            data[0..12].copy_from_slice(b"image/c.hg2\0");
            data[16..32].copy_from_slice(b"image/absent.hg2");
            Ok(Some(Program::from_code(&self.0, data, 96)?))
        }
    }

    struct Files(Vec<u8>);
    impl ResourceServices for Files {
        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            match name {
                b"image/c.hg2" => Ok(Some(self.0.clone())),
                b"image/absent.hg2" => Ok(None),
                _ => panic!("unexpected cursor lookup {name:?}"),
            }
        }
    }

    let mut code = vec![];
    for (op, values) in [
        (0x25, &[0xcafe_babe][..]),
        (0x25, &[0]),
        (0x25, &[0x8100_0001]),
        (0x25, &[0x7200_0002]),
        (0x2b, &[395, 12]),
        (0x25, &[80]),
        (0x2b, &[71, 4]),
        (0x26, &[104]),
        (0x25, &[16]),
        (0x25, &[0]),
        (0x25, &[0]),
        (0x2b, &[395, 12]),
        (0x26, &[104]),
        (0x3b, &[]),
    ] {
        code.push(op);
        for value in values.iter().copied() {
            code.push(3);
            code.extend(u32::to_le_bytes(value));
        }
    }

    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 0)),
        expression::State::new(0),
        InputFactory {
            fallback: KcsFactory::new(Source(code)),
        },
    );
    let projection = Projection::default();
    let transform = *Transform::default().matrix();
    rt.set_frame_view(projection.clone(), transform, TextureFilter::Linear);
    let drawing = rt.compositor_mut().drawing_mut();
    drawing.enabled = true;
    drawing.target = Some(RenderTarget::new([16, 12])?);
    let mut pixels = [0; 16];
    pixels[..4].copy_from_slice(&[0, 0, 255, 255]);
    rt.set_resources(Files(katze_system2::hg2::encode_frame([2, 2], &pixels)?));
    let device = Rc::new(RefCell::new(DeviceState::new([5, 6])));
    rt.set_input_device(Device(device.clone()))?;
    rt.input_coordinates_mut().logical_size = [16, 12];
    rt.create(0, kind::INPUT, b"input", b"")?;
    let caller = rt.create(0, kind::ORDINARY, b"caller", b"")?;
    rt.update(0, 0)?;

    assert!(rt.uses_image_cursor());
    let mut output = RenderTarget::new([16, 12])?;
    assert!(rt.render_frame(&mut output, &projection, transform, TextureFilter::Linear)?);
    assert_eq!(output.pixel(4, 4), Some(0xffff_0000));
    assert_eq!(output.pixel(5, 4), Some(0));
    assert_eq!(output.pixel(5, 6), Some(0));
    let mut composed = katze_system2::host::graphics::Surface::new([16, 12])?;
    assert!(rt.with_frame(|draw, canvas| SoftwareBackend::render_into(
        draw,
        canvas,
        &mut composed
    ))?);
    assert_eq!(output.tiles()[0].as_ref(), Some(&composed));

    device.borrow_mut().cursor = 0;
    assert!(rt.compose_frame()?.commands.is_empty());
    device.borrow_mut().cursor = 1;
    device.borrow_mut().sample.active = 0;
    assert!(rt.compose_frame()?.commands.is_empty());
    device.borrow_mut().sample.active = 1;
    device.borrow_mut().sample.position = [16, 6];
    assert!(rt.compose_frame()?.commands.is_empty());
    device.borrow_mut().sample.position = [5, 6];
    assert!(!rt.compose_frame()?.commands.is_empty());

    rt.send_message(0, caller, [0; 4])?;
    assert!(!rt.uses_image_cursor());
    assert!(rt.compose_frame()?.commands.is_empty());
    let vm = rt.object(caller).unwrap().interpreter().unwrap();
    assert_eq!(vm.read(104, 4)?, 0xcafe_babeu32.to_le_bytes());
    assert!(vm.stack_bytes().is_empty());
    Ok(())
}

#[test]
fn ordinary_input_wrappers_query_devices_and_update_regions_and_key_switches() -> Result<()> {
    use katze_system2::{
        graphics::Rect,
        runtime::{KcsFactory, ProgramLoader, RootObject},
    };

    struct Source(Vec<u8>);
    impl ProgramLoader for Source {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut data = vec![0; 160];
            for (i, value) in [1i32, 2, 5, 6].into_iter().enumerate() {
                data[65 + i * 4..69 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
            for (i, value) in [0i32, 0, 1, 1].into_iter().enumerate() {
                data[85 + i * 4..89 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
            Ok(Some(Program::from_code(&self.0, data, 128)?))
        }
    }
    let mut code = Vec::new();
    let emit = |code: &mut Vec<u8>, opcode, words: &[u32]| {
        code.push(opcode);
        for word in words {
            code.push(3);
            code.extend(word.to_le_bytes());
        }
    };
    let call = |code: &mut Vec<u8>, native, words: &[u32]| {
        for word in words {
            emit(code, 0x25, &[*word]);
        }
        emit(code, 0x2b, &[native, words.len() as u32 * 4]);
    };
    let wait = |code: &mut Vec<u8>| {
        call(code, 71, &[128]);
        emit(code, 0x26, &[124]);
    };
    emit(&mut code, 0x25, &[0xdead_beef]);
    call(&mut code, 144, &[0x8765_4321, u32::MAX, 85, 2]);
    call(&mut code, 285, &[0]);
    emit(&mut code, 0x26, &[0]);
    for (native, args) in [
        (146, [9, 77]),
        (148, [9, 0x1234_5678]),
        (147, [9, 65]),
        (154, [0, 180]),
        (403, [0, 0]),
    ] {
        call(&mut code, native, &args);
    }
    call(&mut code, 401, &[0]);
    call(&mut code, 396, &[u32::MAX]);
    emit(&mut code, 0x26, &[16]);
    call(&mut code, 150, &[0x1000a, 0x20014, 7]);
    wait(&mut code);
    call(&mut code, 285, &[0]);
    emit(&mut code, 0x26, &[4]);
    call(&mut code, 147, &[9, 0]);
    call(&mut code, 286, &[u32::MAX]);
    call(&mut code, 404, &[u32::MAX]);
    call(&mut code, 937, &[u32::MAX]);
    wait(&mut code);
    call(&mut code, 145, &[9]);
    call(&mut code, 285, &[79]);
    emit(&mut code, 0x26, &[8]);
    emit(&mut code, 0x26, &[12]);
    emit(&mut code, 0x3b, &[]);
    for budget in [1, 4096] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(RootObject::new(1, 0)),
            expression::State::new(0),
            InputFactory {
                fallback: KcsFactory::new(Source(code.clone())),
            },
        );
        rt.set_budget(budget, 1 << 20);
        rt.set_input_pointer_control(true);
        let input = rt.create(0, kind::INPUT, b"input", b"")?;
        rt.input_state_mut(input).unwrap().set_update_rate(120);
        rt.input_state_mut(input)
            .unwrap()
            .pointer_mut()
            .cursor_shown = 0x8000_0001;
        let caller = rt.create(0, kind::ORDINARY, b"caller", b"")?;
        let device = Rc::new(RefCell::new(DeviceState::new([20, 30])));
        device.borrow_mut().key_mask = 2;
        device.borrow_mut().wheel_repeats = 7;
        rt.set_input_device(Device(device.clone()))?;
        rt.input_coordinates_mut().logical_size = [100, 100];
        let mut bytes = region(9, [40, 40, 50, 50]);
        rt.dispatch_command_with_buffers(
            0,
            Command {
                sender: 0,
                target: input,
                code: 127,
                data: [0; 3],
            },
            &mut CommandBuffers {
                data1: Some(&mut bytes),
                ..Default::default()
            },
        )?;
        rt.update(0, 0)?;
        let current = rt.input_state(input).unwrap();
        let entry = current.regions().get(9).unwrap();
        assert_eq!(entry.priority(), 77);
        assert_eq!(entry.event_mask(), 0x1234_5678);
        assert_eq!(entry.bounds(), Rect::from_xywh(1, 2, 4, 4));
        let own = current.regions().get(caller).unwrap();
        assert_eq!(own.priority(), -1);
        assert_eq!(own.event_mask(), 2);
        assert_eq!(own.bounds(), Rect::from_xywh(0, 0, 1, 1));
        assert_eq!(own.enabled(), 1);
        assert_eq!(own.selection_enabled(), 1);
        assert_eq!(current.keys().enabled()[0], 0);
        assert_eq!(current.pointer().idle_limit, 360);
        assert_eq!(current.pointer().cursor_requested, 0);
        assert_eq!(current.pointer().move_target, [10, 20]);
        assert_eq!(current.pointer().move_remaining, 7);
        assert_eq!(
            rt.object(caller)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)?,
            2u32.to_le_bytes()
        );
        rt.send_message(0, caller, [0; 4])?;
        let current = rt.input_state(input).unwrap();
        assert_eq!(current.regions().get(9).unwrap().bounds(), Rect::default());
        assert_eq!(current.keys().enabled(), &[1; 80]);
        assert_eq!(current.pointer().position, [-1; 2]);
        assert_eq!(current.pointer().forced, 1);
        assert_eq!(device.borrow().wheel_repeats, 0);
        assert_eq!(device.borrow().key_mask, 2);
        assert_eq!(
            rt.object(caller)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(4, 4)?,
            2u32.to_le_bytes()
        );
        rt.send_message(0, caller, [0; 4])?;
        let current = rt.input_state(input).unwrap();
        assert!(current.regions().get(caller).is_none());
        assert!(current.regions().get(9).is_some());
        let vm = rt.object(caller).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(8, 4)?, 0u32.to_le_bytes());
        assert_eq!(vm.read(12, 4)?, 0xdead_beefu32.to_le_bytes());
        assert_eq!(vm.read(16, 4)?, 0x8000_0001u32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
        let queries = &device.borrow().queries;
        assert!(queries.windows(4).any(|q| q
            == [
                (79, 0, false),
                (79, 1, false),
                (79, 2, false),
                (79, 3, false)
            ]));
    }
    Ok(())
}

struct Factory {
    mode: Rc<Cell<u32>>,
    records: Records,
}

#[test]
fn ordinary_text_entry_tracks_pending_accepted_and_cancelled_requests() -> Result<()> {
    use katze_system2::runtime::{ProgramLoader, TaskRuntime};

    struct Source(Vec<u8>);
    impl ProgramLoader for Source {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut data = vec![0x5a; 192];
            data[0..5].copy_from_slice(b"seed\0");
            data[8..10].copy_from_slice(b"B\0");
            Ok(Some(Program::from_code(&self.0, data, 128)?))
        }
    }

    let mut code = Vec::new();
    let emit = |code: &mut Vec<u8>, opcode, words: &[u32]| {
        code.push(opcode);
        for word in words {
            code.push(3);
            code.extend(word.to_le_bytes());
        }
    };
    let call = |code: &mut Vec<u8>, native, words: &[u32]| {
        for word in words {
            emit(code, 0x25, &[*word]);
        }
        emit(code, 0x2b, &[native, words.len() as u32 * 4]);
    };
    let query = |code: &mut Vec<u8>, status_at, text_at| {
        call(code, 152, &[u32::MAX]);
        emit(code, 0x26, &[status_at]);
        call(code, 153, &[text_at]);
    };
    let wait = |code: &mut Vec<u8>| {
        call(code, 71, &[96]);
        emit(code, 0x26, &[124]);
    };
    emit(&mut code, 0x25, &[0xcafe_babe]);
    query(&mut code, 24, 40);
    call(&mut code, 151, &[0]);
    query(&mut code, 28, 48);
    wait(&mut code);
    call(&mut code, 151, &[8]);
    wait(&mut code);
    query(&mut code, 32, 56);
    call(&mut code, 151, &[8]);
    wait(&mut code);
    query(&mut code, 36, 64);
    emit(&mut code, 0x26, &[72]);
    emit(&mut code, 0x3b, &[]);

    for budget in [1, 4096] {
        let mut rt = TaskRuntime::with_programs(Source(code.clone()), 0, Default::default())?;
        rt.set_budget(budget, 1 << 20);
        rt.set_input_device(Device(Rc::new(RefCell::new(DeviceState::new([0, 0])))))?;
        let (mut rt, tasks) = rt.start_game_with_tasks(b"caller")?;
        let caller = tasks.entry;
        rt.update(0, 0)?;
        let first = rt.text_entry_request().unwrap();
        assert_eq!(&*first, "seed");

        rt.send_message(0, caller, [0; 4])?;
        let second = rt.text_entry_request().unwrap();
        assert_eq!(&*second, "B");
        assert!(!rt.complete_text_entry(&first, Some("late"))?);
        assert!(rt.complete_text_entry(&second, Some("あ"))?);
        assert!(!rt.complete_text_entry(&second, None)?);

        rt.send_message(0, caller, [0; 4])?;
        let third = rt.text_entry_request().unwrap();
        assert_eq!(&*third, "B");
        assert!(!rt.complete_text_entry(&second, Some("late"))?);
        assert!(rt.complete_text_entry(&third, None)?);
        assert!(rt.text_entry_request().is_none());

        rt.send_message(0, caller, [0; 4])?;
        let vm = rt.object(caller).unwrap().interpreter().unwrap();
        for (at, status) in [(24, 2u32), (28, 0), (32, 1), (36, 2)] {
            assert_eq!(vm.read(at, 4)?, status.to_le_bytes());
        }
        for (at, text) in [
            (40, b"\0Z".as_slice()),
            (48, b"seed\0Z"),
            (56, b"\x82\xa0\0Z"),
            (64, b"B\0Z"),
        ] {
            assert_eq!(vm.read(at, text.len())?, text);
        }
        assert_eq!(vm.read(72, 4)?, 0xcafe_babeu32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
    }
    Ok(())
}

struct Receiver {
    base: BaseObject,
    id: u32,
    mode: Rc<Cell<u32>>,
    records: Records,
}

fn send(host: &mut dyn Host, sender: u32, code: u32, a: u32, b: u32) -> Result<u32> {
    let mut output = 0x13579bdf;
    host.dispatch_task_command(
        Command {
            sender,
            target: 1,
            code,
            data: [a, b, 0],
        },
        &mut CommandBuffers::default(),
        &mut output,
    )?;
    Ok(output)
}

fn region(id: u32, bounds: [i32; 4]) -> [u8; 60] {
    let mut bytes = [0; 60];
    for (index, value) in [(0, id), (2, 2)].into_iter().chain(
        bounds
            .into_iter()
            .enumerate()
            .map(|(i, n)| (i + 3, n as u32)),
    ) {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, object_kind: u32, _name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind != kind::BASE {
            return Err(Error::new(0, "测试对象类型无效"));
        }
        Ok(Box::new(Receiver {
            base: BaseObject::new(),
            id,
            mode: self.mode.clone(),
            records: self.records.clone(),
        }))
    }
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
    fn dispatch_command(
        &mut self,
        kind: u32,
        packet: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            packet,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        packet: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if !matches!(packet.code, 4..=8 | 22) {
            return self
                .base
                .dispatch_command(kind, packet, output, host, budget);
        }
        let selected = send(host, self.id, 134, self.id, 0)?;
        let raw = send(host, self.id, 137, 0, 0)?;
        let mut points = Vec::new();
        if packet.code == 6 {
            points.extend_from_slice(buffers.data0.as_deref().unwrap());
            points.extend_from_slice(buffers.data1.as_deref().unwrap());
        } else if packet.code == 22 {
            let mut shown = [0xff; 8];
            host.dispatch_task_command(
                Command {
                    sender: self.id,
                    target: 1,
                    code: 149,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut shown),
                    external: None,
                },
                &mut 0,
            )?;
            assert_eq!(&shown[4..], &[0xff; 4]);
            points.extend_from_slice(&shown[..4]);
        } else if self.mode.get() == 8 && packet.code == 4 {
            let mut vm = Interpreter::new(Program::from_code(&[], vec![0; 8], 16)?, vec![])?;
            let arguments: Vec<_> = [0u32, 4].into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(host.call(149, &arguments, &mut vm)?, NativeReturn::Continue);
            points.extend_from_slice(&vm.read(0, 8)?);
        }
        self.records.borrow_mut().push(Record {
            packet: *packet,
            selected,
            raw,
            points,
        });
        match (self.mode.get(), packet.code, self.id) {
            (1, 4, 2) => {
                send(host, self.id, 24, 0, 0x12345678)?;
                send(host, self.id, 29, 0, 7)?;
                send(host, self.id, 132, self.id, 0)?;
                let mut bytes = region(3, [0; 4]);
                host.dispatch_task_command(
                    Command {
                        sender: self.id,
                        target: 1,
                        code: 127,
                        data: [0; 3],
                    },
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut bytes),
                        external: None,
                    },
                    &mut 0,
                )?;
                *output = 1;
            }
            (2 | 3, 4, _) => {
                send(host, self.id, 151, 1, 0)?;
                *output = 1;
                if self.mode.get() == 3 {
                    send(host, self.id, 24, 0, 0x87654321)?;
                    send(host, self.id, 155, 0, 0)?;
                }
            }
            (4, 4, _) => {
                let mut vm = Interpreter::new(Program::from_code(&[], vec![], 16)?, vec![])?;
                assert_eq!(
                    host.call(67, &1u32.to_le_bytes(), &mut vm)?,
                    NativeReturn::Continue
                );
                assert_eq!(send(host, self.id, 137, 0, 0)?, 0x13579bdf);
                *output = 0;
            }
            (5, 8, _) => {
                send(host, self.id, 133, self.id, 99)?;
            }
            (5, 6, 2) => {
                let previous = buffers.data0.as_deref_mut().unwrap();
                previous[..4].copy_from_slice(&5i32.to_le_bytes());
                previous[4..].copy_from_slice(&5i32.to_le_bytes());
                let current = buffers.data1.as_deref_mut().unwrap();
                current[..4].copy_from_slice(&(-7i32).to_le_bytes());
                current[4..].copy_from_slice(&17i32.to_le_bytes());
            }
            (6, 4, _) => {}
            (7, 22, 2) if packet.data[1] == 0 => {
                send(host, self.id, 150, 0, 0)?;
                *output = 0;
            }
            (9, 22, 2) => {
                send(host, self.id, 150, 0, 0)?;
                send(host, self.id, 159, 0, 0)?;
            }
            (11, 22, 2) => {
                send(host, self.id, 155, 0, 0)?;
            }
            (12, 6, 2) => {
                send(host, self.id, 139, 5, 50 | 60 << 16)?;
            }
            _ => *output = 0,
        }
        Ok(CommandOutcome::finished(false))
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
}

type TestRuntime = Runtime<InputFactory<Factory>>;

fn runtime(mode: u32) -> (TestRuntime, Rc<Cell<u32>>, Records) {
    let mode = Rc::new(Cell::new(mode));
    let records = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        expression::State::new(0),
        InputFactory {
            fallback: Factory {
                mode: mode.clone(),
                records: records.clone(),
            },
        },
    );
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(runtime.create(0, kind::BASE, b"first", b"").unwrap(), 2);
    assert_eq!(runtime.create(0, kind::BASE, b"second", b"").unwrap(), 3);
    (runtime, mode, records)
}

fn register(runtime: &mut TestRuntime, id: u32, priority: u32, bounds: [i32; 4]) {
    let mut bytes = region(id, bounds);
    runtime
        .dispatch_command_with_buffers(
            0,
            Command {
                sender: 0,
                target: 1,
                code: 127,
                data: [priority, 0, 0],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
                external: None,
            },
        )
        .unwrap();
}

fn event(mask: u32) -> KeyEvent {
    KeyEvent {
        kind: KeyEventKind::Change,
        code: 4,
        key: 0,
        pressed: 1,
        mask,
        point: [0; 2],
    }
}

#[test]
fn input_callbacks_query_live_state_and_append_a_same_dispatch_target() -> Result<()> {
    let (mut runtime, _, records) = runtime(1);
    register(&mut runtime, 2, 10, [0; 4]);
    runtime.host_mut().task_id = 0;
    runtime.host_mut().task_time = 99;
    runtime.host_mut().task_origin = 88;
    runtime.set_budget(1, 1024);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    let state = runtime.input_state(1).unwrap();
    assert_eq!(
        state
            .regions()
            .records()
            .iter()
            .map(|r| r.id())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(state.regions().get(2).unwrap().event_mask(), 0);
    assert_eq!(state.events().captures()[0], 3);
    assert_eq!(runtime.object(1).unwrap().control().property, 0x12345678);
    assert_eq!(runtime.object(1).unwrap().clock().paused, 7);
    assert_eq!(
        (
            runtime.host().task_id,
            runtime.host().task_time,
            runtime.host().task_origin
        ),
        (0, 99, 88)
    );
    let records = records.borrow();
    assert_eq!(
        records.iter().map(|r| r.packet.target).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(
        records
            .iter()
            .all(|r| r.raw == 2 && r.packet.sender == 1 && r.packet.data == [0, 1, 0])
    );
    Ok(())
}

#[test]
fn scanning_callbacks_modify_later_keys_and_restore_all_state_on_errors() -> Result<()> {
    let (mut runtime, mode, _) = runtime(2);
    register(&mut runtime, 2, 10, [0; 4]);
    let mut queried = Vec::new();
    assert!(!runtime.scan_input_keys(1, [0; 2], |key, group| {
        queried.push((key, group));
        Ok(KeySample {
            mask: if key < 2 && group == 0 { 2 } else { 0 },
            repeat: 1,
        })
    })?);
    assert!(queried.iter().all(|&(key, _)| key != 1));
    assert_eq!(queried.len(), 316);
    let state = runtime.input_state(1).unwrap();
    assert_eq!(state.keys().enabled()[1], 0);
    assert_eq!(state.keys().records()[0][0].count, 1);
    assert_eq!(state.keys().records()[1][0].count, 0);
    mode.set(3);
    let error = runtime
        .scan_input_keys(1, [0; 2], |key, group| {
            Ok(KeySample {
                mask: if key == 0 && group == 0 { 3 } else { 0 },
                repeat: 1,
            })
        })
        .unwrap_err();
    assert!(error.message.contains("155"));
    assert_eq!(
        runtime.input_state(1).unwrap().keys().records()[0][0].mask,
        3
    );
    assert_eq!(runtime.input_state(1).unwrap().events().raw_state(), 3);
    assert_eq!(runtime.object(1).unwrap().control().property, 0x87654321);
    assert_eq!(
        runtime.dispatch_command(
            0,
            Command {
                sender: 0,
                target: 1,
                code: 137,
                data: [0; 3]
            }
        )?,
        3
    );
    mode.set(6);
    assert!(!runtime.dispatch_input_key(1, event(2))?);
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 0);
    mode.set(0);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    assert!(
        runtime
            .dispatch_input_key(2, event(2))
            .unwrap_err()
            .message
            .contains("输入对象")
    );
    assert!(runtime.object(2).is_some());
    Ok(())
}

#[test]
fn pointer_notifications_keep_selection_timing_and_share_mutable_buffers() -> Result<()> {
    let (mut runtime, _, records) = runtime(5);
    register(&mut runtime, 2, 10, [0, 0, 2, 2]);
    register(&mut runtime, 3, 0, [5, 5, 8, 8]);
    runtime.update_input_selection(1, Some([1, 1]))?;
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(2)
            .unwrap()
            .selected(),
        1
    );
    runtime.update_input_selection(1, None)?;
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(2)
            .unwrap()
            .selected(),
        0
    );
    runtime.dispatch_input_motion(
        1,
        PointerMotion {
            code: 6,
            previous: [1, 1],
            current: [9, 9],
        },
    )?;
    let records = records.borrow();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.packet.target, r.packet.code, r.selected))
            .collect::<Vec<_>>(),
        [(2, 7, 1), (2, 8, 1), (2, 6, 0), (3, 6, 0)]
    );
    let points: Vec<_> = records[3]
        .points
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(points, [5, 5, -7, 17]);
    Ok(())
}

#[test]
fn input_deletion_in_a_callback_is_deferred_and_recreation_has_fresh_state() -> Result<()> {
    let (mut runtime, _, _) = runtime(4);
    register(&mut runtime, 2, 10, [0; 4]);
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("自动设备更新")
    );
    assert!(runtime.dispatch_input_key(1, event(2))?);
    assert_eq!(runtime.object(1).unwrap().control().deletion_requested, 1);
    assert!(runtime.update(2, 1)?);
    assert!(runtime.object(1).is_none());
    assert!(runtime.input_state(1).is_none());
    assert_eq!(runtime.create(0, kind::INPUT, b"new", b"")?, 1);
    let state = runtime.input_state(1).unwrap();
    assert!(state.regions().records().is_empty());
    assert_eq!(state.keys().enabled(), &[1; 80]);
    assert_eq!(state.events().captures(), &[0; 80]);
    assert_eq!(state.events().raw_state(), 0);
    assert!(
        runtime
            .create(0, kind::INPUT, b"context", b"nonempty")
            .is_err()
    );
    Ok(())
}

struct DeviceState {
    sample: PointerSample,
    positions: VecDeque<[i32; 2]>,
    pointer_calls: usize,
    fail_pointer: Option<usize>,
    key_mask: u32,
    queries: Vec<(u32, u32, bool)>,
    wheel: u32,
    wheel_repeats: u32,
    fail_wheel_cancel: bool,
    cursor: u32,
    shows: Vec<u32>,
    calls: Vec<&'static str>,
    foreground: bool,
    moves: Vec<[i32; 2]>,
    clear_calls: usize,
    fail_clear: bool,
}

impl DeviceState {
    fn new(position: [i32; 2]) -> Self {
        Self {
            sample: PointerSample {
                position,
                active: 1,
            },
            positions: VecDeque::new(),
            pointer_calls: 0,
            fail_pointer: None,
            key_mask: 0,
            queries: Vec::new(),
            wheel: 0,
            wheel_repeats: 0,
            fail_wheel_cancel: false,
            cursor: 1,
            shows: Vec::new(),
            calls: Vec::new(),
            foreground: true,
            moves: Vec::new(),
            clear_calls: 0,
            fail_clear: false,
        }
    }
}

struct Device(Rc<RefCell<DeviceState>>);

#[test]
fn standard_startup_rejects_incomplete_host_setup_before_calling_services() -> Result<()> {
    use katze_system2::{
        host::resources::{ResourceMode, ResourceServices},
        session::{GameSetup, StartupError},
    };

    struct Entry;
    impl ResourceServices for Entry {
        fn resolve(&mut self, _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
            panic!("incomplete startup must not resolve the entry script")
        }

        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("incomplete startup must not read the entry script")
        }
    }

    for missing in [
        StartupError::MissingResources,
        StartupError::MissingInput,
        StartupError::MissingCanvas,
    ] {
        let device = Rc::new(RefCell::new(DeviceState::new([12, 34])));
        let mut game = GameSetup::new(0, 0)?;
        if missing != StartupError::MissingResources {
            game.set_resources(Entry);
        }
        if missing != StartupError::MissingInput {
            game.set_input_device(Device(device.clone()))?;
        }
        if missing != StartupError::MissingCanvas {
            game.configure_canvas([64, 48])?;
        } else {
            assert!(game.configure_canvas([64, 0]).is_err());
        }
        let preview = game.validate_startup().unwrap_err();
        assert_eq!(preview, missing);
        assert_eq!(game.start_game(b"main").err(), Some(preview));
        assert_eq!(device.borrow().pointer_calls, 0);
    }
    Ok(())
}

fn packed_kcs(code: &[u8]) -> Vec<u8> {
    use std::io::Write;

    let mut program = b"KCS\0".to_vec();
    let len = code.len() as u32;
    for word in [2, 0, 52, len, 52 + len, 0, 0, 0, 64, 52 + len, 0, len] {
        program.extend(word.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(program, flate2::Compression::fast());
    encoder.write_all(code).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn public_session_reports_startup_and_update_failures_by_stage() -> Result<()> {
    use katze_system2::{
        host::resources::{ResourceMode, ResourceServices},
        session::{AdvanceError, FrameTime, GameSetup, StartupError},
    };

    struct Files {
        entry: Result<Vec<u8>>,
        language_reads: usize,
    }

    impl ResourceServices for Files {
        fn resolve(&mut self, _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
            Ok(b"main.kcs".to_vec())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            match (name, mode) {
                (b"main.kcs", ResourceMode::ArchiveFirst) => self.entry.clone().map(Some),
                (b"options.txt", ResourceMode::File) => {
                    self.language_reads += 1;
                    if self.language_reads == 1 {
                        Err(Error::new(14, "temporary translation read failure"))
                    } else {
                        Ok(Some(Vec::new()))
                    }
                }
                _ => panic!("unexpected resource: {name:?}, {mode:?}"),
            }
        }
    }

    let prepare = |entry| -> Result<_> {
        let device = Rc::new(RefCell::new(DeviceState::new([0; 2])));
        let mut game = GameSetup::new(0, 0)?;
        let startup = br#"<?xml version="1.0" encoding="UTF-8"?><document><SCREEN><width>64</width><height>48</height></SCREEN><language><OPTTABLE>options.txt</OPTTABLE></language></document>"#;
        let size = game.configure_startup(startup, None, None)?;
        game.configure_canvas(size)?;
        game.set_input_device(Device(device.clone()))?;
        game.set_resources(Files {
            entry,
            language_reads: 0,
        });
        Ok((game, device))
    };
    let entry_error = Error::new(17, "entry read failed");
    let (broken, _) = prepare(Err(entry_error.clone()))?;
    assert_eq!(
        broken.start_game(b"main").err(),
        Some(StartupError::StartTasks(entry_error))
    );

    let mut code = Vec::new();
    for (op, args) in [
        (0x25, &[1u32][..]),
        (0x25, &[1][..]),
        (0x2b, &[1053, 8][..]),
    ] {
        code.push(op);
        for arg in args {
            code.push(3);
            code.extend(arg.to_le_bytes());
        }
    }
    code.push(0x3b);
    let (game, device) = prepare(Ok(packed_kcs(&code)))?;
    let mut game = game.start_game(b"main").unwrap();
    let time = |tick| FrameTime {
        tick,
        increment: 1,
        system_time_ms: tick,
    };
    game.advance(time(1)).unwrap();
    let before = device.borrow().pointer_calls;
    device.borrow_mut().fail_pointer = Some(before + 1);
    let translation_error = Error::new(14, "temporary translation read failure");
    assert_eq!(
        game.advance(time(2)),
        Err(AdvanceError::LanguageRefresh(translation_error))
    );
    assert_eq!(device.borrow().pointer_calls, before);
    assert_eq!(
        game.advance(time(2)),
        Err(AdvanceError::TaskUpdate(Error::new(0, "合成设备查询失败")))
    );
    assert_eq!(device.borrow().pointer_calls, before + 1);
    Ok(())
}

#[test]
fn public_session_uses_host_resources_and_delivers_frame_and_pcm()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use katze_system2::{
        host::{
            diagnostics::{ScriptLocation, ScriptPosition},
            resources::{ResourceMode, ResourceServices},
        },
        kcs::ScriptRandom,
        runtime::TaskRuntime,
        session::{FrameOutcome, FrameTime, GameRuntime, GameSetup},
    };

    struct Entry;
    impl ResourceServices for Entry {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!((category, name), (b"kcs".as_slice(), b"main".as_slice()));
            Ok(b"main.kcs".to_vec())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(name, b"main.kcs");
            assert_eq!(mode, ResourceMode::ArchiveFirst);
            let mut code = Vec::new();
            for (slot, native) in [33u32, 34, 33, 34].into_iter().enumerate() {
                for (op, args) in [
                    (0x25, &[slot as u32][..]),
                    (0x2b, &[native, 0][..]),
                    (0x2b, &[211, 8][..]),
                ] {
                    code.push(op);
                    for value in args {
                        code.push(3);
                        code.extend(value.to_le_bytes());
                    }
                }
            }
            code.push(0x3b);
            Ok(Some(packed_kcs(&code)))
        }
    }

    let mut game = GameSetup::standard()?;
    let seed = 8_713;
    game.set_script_random_seed(seed);
    assert!(game.window_preferences().is_none());
    assert!(!game.set_image_import_directory(Some(vec![65])));
    let startup = br#"<?xml version="1.0" encoding="UTF-8"?><document><SCREEN><width>64</width><height>48</height></SCREEN><GAME><autoselect>1</autoselect></GAME></document>"#;
    let user = br#"<?xml version="1.0" encoding="UTF-8"?><document><WINDOW><width>256</width><height>192</height><full_width>160</full_width><fullscreen>3</fullscreen></WINDOW><SOUND><stereo>0</stereo></SOUND><ETC><imgimport>initial</imgimport></ETC></document>"#;
    let logical_size = game.configure_startup(startup, None, Some(user))?;
    assert_eq!(logical_size, [64, 48]);
    let prefs = game.window_preferences().unwrap();
    assert_eq!(prefs.window_size, [256, 192]);
    assert_eq!(prefs.fullscreen_size, [64, 48]);
    assert!(prefs.fullscreen);
    let initial: Vec<_> = "initial".encode_utf16().collect();
    assert_eq!(game.image_import_directory(), Some(initial.as_slice()));

    let invalid = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><GAME><autoselect>2</autoselect></GAME><TTS><id>{}</id></TTS></document>",
        "a".repeat(64)
    );
    assert!(
        game.configure_startup(startup, None, Some(invalid.as_bytes()))
            .is_err()
    );
    assert_eq!(game.window_preferences(), Some(prefs));
    assert_eq!(game.image_import_directory(), Some(initial.as_slice()));
    let last = vec![65; 259];
    assert!(game.set_image_import_directory(Some(last.clone())));
    assert!(!game.set_image_import_directory(Some(vec![65; 260])));
    assert!(!game.set_image_import_directory(Some(vec![65, 0])));
    assert_eq!(game.image_import_directory(), Some(last.as_slice()));

    game.set_resources(Entry);
    let device = Rc::new(RefCell::new(DeviceState::new([12, 34])));
    game.set_input_device(Device(device))?;
    game.configure_canvas(logical_size)?;
    let mut game: GameRuntime = game.start_game(b"main")?;
    assert_eq!(game.window_preferences(), Some(prefs));
    assert_eq!(game.image_import_directory(), Some(last.as_slice()));
    assert!(game.script_positions().eq([ScriptPosition {
        task_id: 3,
        location: ScriptLocation::Kcs { byte_offset: 0 },
    }]));
    assert_eq!(
        game.advance(FrameTime {
            tick: 1,
            increment: 1,
            system_time_ms: 16
        })?,
        FrameOutcome::Running {
            interface_language_refreshed: false
        },
    );
    assert_eq!(game.with_frame(|_, canvas| Ok(canvas.size()))?, [64, 48]);
    let mut pcm = [1.0; 4];
    game.mix_audio_stereo(48_000, &mut pcm)?;
    assert_eq!(pcm, [0.0; 4]);
    let runtime = TaskRuntime::from_session(game);
    let mut random = ScriptRandom::seeded(seed);
    assert_eq!(
        &runtime.numbers().shared[..4],
        &[
            random.next_integer() as i32,
            random.next_float().to_bits() as i32,
            random.next_integer() as i32,
            random.next_float().to_bits() as i32,
        ]
    );
    assert_eq!(runtime.input_coordinates().logical_size, [64, 48]);
    assert_eq!(runtime.system_settings().unwrap().auto_select, 1);
    assert_eq!(runtime.system_settings().unwrap().fullscreen, 3);
    let platform = runtime.platform_settings().unwrap();
    assert_eq!(platform.sound_stereo, 0);
    assert_eq!(platform.directories[2], Some(last));
    Ok(())
}

#[test]
fn public_session_delivers_window_requests_to_the_host_event_loop() -> Result<()> {
    use katze_system2::{
        host::{
            platform::{MenuItemRequest, WindowAction, WindowCloseRequest},
            resources::{ResourceMode, ResourceServices},
        },
        runtime::TaskRuntime,
        session::{FrameTime, GameSetup},
    };

    struct Entry;
    impl ResourceServices for Entry {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }

        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            let mut code = Vec::new();
            for (native, argument) in [
                (9u32, Some(1u32)),
                (393, Some(0)),
                (628, Some(2)),
                (845, None),
                (847, None),
                (1054, None),
                (628, Some(3)),
                (393, Some(u32::MAX)),
                (9, Some(0)),
            ] {
                if let Some(value) = argument {
                    code.extend([0x25, 3]);
                    code.extend(value.to_le_bytes());
                }
                code.extend([0x2b, 3]);
                code.extend(native.to_le_bytes());
                code.push(3);
                code.extend(if argument.is_some() { 4u32 } else { 0 }.to_le_bytes());
            }
            code.push(0x3b);
            Ok(Some(packed_kcs(&code)))
        }
    }

    let mut setup = GameSetup::new(0, 0)?;
    let size = setup.configure_startup(
        br#"<?xml version="1.0" encoding="UTF-8"?><document><SCREEN><width>64</width><height>48</height></SCREEN></document>"#,
        None,
        None,
    )?;
    setup.set_resources(Entry);
    setup.configure_canvas(size)?;
    setup.set_input_device(Device(Rc::new(RefCell::new(DeviceState::new([12, 34])))))?;
    let mut game = setup.start_game(b"main").unwrap();
    let entry = game.script_positions().next().unwrap().task_id;
    game.advance(FrameTime {
        tick: 1,
        increment: 1,
        system_time_ms: 16,
    })
    .unwrap();

    let close = game.take_window_close_requests();
    assert_eq!(
        close,
        [false, true].map(|notify| WindowCloseRequest {
            task_id: entry,
            notify
        })
    );
    assert!(game.take_window_close_requests().is_empty());
    let menu = game.take_menu_item_requests();
    assert_eq!(
        menu,
        [0, u32::MAX].map(|value| MenuItemRequest {
            task_id: entry,
            command_id: 0x77,
            value
        })
    );
    assert!(game.take_menu_item_requests().is_empty());
    let actions: Vec<_> = game
        .take_window_requests()
        .into_iter()
        .map(|request| request.action)
        .collect();
    assert_eq!(
        actions,
        [
            WindowAction::RebuildMenu,
            WindowAction::ToggleFullscreen,
            WindowAction::CloseFromMenu,
            WindowAction::NotifyTasks,
            WindowAction::RebuildMenu
        ]
    );
    assert!(game.pending_window_requests().is_empty());
    assert_eq!(game.debug_mode(), 3);
    game.report_fullscreen(true);
    assert!(game.window_preferences().unwrap().fullscreen);
    game.prepare_menu_close()?;
    game.notify_window_tasks()?;
    assert_eq!(
        TaskRuntime::from_session(game).host().system_settings_dirty,
        1
    );
    Ok(())
}

#[test]
fn game_runtime_boots_the_standard_tasks_and_supplies_the_first_frame_clock() -> Result<()> {
    use katze_system2::{
        language::{LanguageChangeRequest, LanguageTarget},
        memory::SharedMemory,
        runtime::{ProgramLoader, TaskRuntime},
        session::{FrameOutcome, FrameTime},
    };

    struct Entry(Program);
    impl ProgramLoader for Entry {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            Ok((name == b"main").then(|| self.0.clone()))
        }
    }

    let mut code = vec![0x2b, 3];
    code.extend(4u32.to_le_bytes());
    code.push(3);
    code.extend(0u32.to_le_bytes());
    code.extend([0x26, 3]);
    code.extend(0u32.to_le_bytes());
    code.push(0x3b);
    let external = SharedMemory::new(vec![0x55; 4])?;
    let mut runtime = TaskRuntime::with_programs(
        Entry(Program::from_code(&code, vec![0; 4], 8)?),
        4,
        external,
    )?;
    let options = |value| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><GAME><autoselect>{value}</autoselect></GAME></document>"
        )
    };
    let startup = options(1);
    let defaults = options(2);
    let user = options(3);
    let startup = startup.as_bytes();
    let defaults = Some(defaults.as_bytes());
    runtime.configure_startup(startup, defaults, None)?;
    assert_eq!(runtime.system_settings().unwrap().auto_select, 2);
    runtime.configure_startup(startup, defaults, Some(user.as_bytes()))?;
    assert_eq!(runtime.system_settings().unwrap().auto_select, 3);
    assert!(
        runtime
            .configure_startup(startup, None, Some(b"<"))
            .is_err()
    );
    assert_eq!(runtime.system_settings().unwrap().auto_select, 3);
    runtime.configure_canvas([64, 48])?;
    assert!(runtime.configure_canvas([u32::MAX, 48]).is_err());
    let device = Rc::new(RefCell::new(DeviceState::new([12, 34])));
    runtime.set_input_device(Device(device.clone()))?;
    let (mut runtime, tasks) = runtime.start_game_with_tasks(b"main")?;
    assert!(runtime.configure_startup(startup, None, None).is_err());
    assert_eq!(device.borrow().pointer_calls, 1);
    let mut pcm = [1.0; 4];
    runtime.mix_audio_stereo(48_000, &mut pcm)?;
    assert_eq!(pcm, [0.0; 4]);
    assert!(runtime.connect_task_audio_output(tasks.entry).is_err());
    let mut mixer = runtime.connect_audio_output()?;
    pcm.fill(1.0);
    mixer.render(48_000, &mut pcm)?;
    assert_eq!(pcm, [0.0; 4]);
    assert!(runtime.mix_audio_stereo(48_000, &mut pcm).is_err());
    for (command, task) in [(34, tasks.input), (35, tasks.sound)] {
        assert_eq!(
            runtime.dispatch_command(
                tasks.entry,
                Command {
                    target: 0,
                    sender: tasks.entry,
                    code: command,
                    data: [0; 3],
                }
            )?,
            task
        );
    }

    let time = FrameTime {
        tick: 120,
        increment: 1,
        system_time_ms: 42_000,
    };
    let request = LanguageChangeRequest {
        task_id: tasks.entry,
        target: LanguageTarget::MainText,
    };
    runtime.host_mut().language_change_requests.push(request);
    assert!(runtime.advance(time).is_err());
    assert_eq!(runtime.host().language_change_requests, [request]);
    assert_eq!(device.borrow().pointer_calls, 1);
    runtime.host_mut().language_change_requests.clear();
    assert_eq!(
        runtime.advance(time)?,
        FrameOutcome::Running {
            interface_language_refreshed: false,
        }
    );
    runtime.with_frame(|_, canvas| {
        assert_eq!(canvas.size(), [64, 48]);
        Ok(())
    })?;
    let vm = runtime.object(tasks.entry).unwrap().interpreter().unwrap();
    assert_eq!(vm.read(0, 4)?.as_ref(), 42_000u32.to_le_bytes());
    assert_eq!(vm.read(0x8000_0000, 4)?.as_ref(), [0x55; 4]);
    runtime.destroy(tasks.sound)?;
    pcm.fill(1.0);
    assert!(runtime.mix_audio_stereo(48_000, &mut pcm[..3]).is_err());
    assert_eq!(pcm, [1.0; 4]);
    runtime.mix_audio_stereo(48_000, &mut pcm)?;
    assert_eq!(pcm, [0.0; 4]);

    assert!(runtime.update(121, 1)?);
    let reused = runtime.create_unloaded_ordinary(0, b"")?;
    assert_eq!(reused, tasks.sound);
    assert!(runtime.connect_audio_output().is_err());
    pcm.fill(1.0);
    runtime.mix_audio_stereo(48_000, &mut pcm)?;
    assert_eq!(pcm, [0.0; 4]);
    Ok(())
}

impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("poll");
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        let mut state = self.0.borrow_mut();
        state.calls.push("pointer");
        state.pointer_calls += 1;
        if state.fail_pointer == Some(state.pointer_calls) {
            return Err(Error::new(0, "合成设备查询失败"));
        }
        let mut sample = state.sample;
        if let Some(position) = state.positions.pop_front() {
            sample.position = position;
        }
        Ok(sample)
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        let mut state = self.0.borrow_mut();
        state.calls.push("key");
        state.queries.push((key, group, repeat));
        Ok(KeySample {
            mask: if key == 0 && group == 0 {
                state.key_mask
            } else {
                0
            },
            repeat: 1,
        })
    }
    fn take_wheel(&mut self) -> Result<u32> {
        let mut state = self.0.borrow_mut();
        state.calls.push("wheel");
        Ok(std::mem::take(&mut state.wheel))
    }
    fn is_foreground(&mut self) -> Result<bool> {
        let mut state = self.0.borrow_mut();
        state.calls.push("foreground");
        Ok(state.foreground)
    }
    fn finish(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("finish");
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("commit");
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        self.0.borrow_mut().calls.push("history");
        Ok(std::array::from_fn(|i| i as u8))
    }
    fn show_cursor(&mut self, visible: u32) -> Result<()> {
        let mut state = self.0.borrow_mut();
        state.calls.push("show");
        state.cursor = visible;
        state.shows.push(visible);
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(self.0.borrow().cursor)
    }
    fn set_pointer(&mut self, position: [i32; 2]) -> Result<()> {
        let mut state = self.0.borrow_mut();
        state.calls.push("set_pointer");
        state.moves.push(position);
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        let mut state = self.0.borrow_mut();
        state.calls.push("clear_keys");
        state.clear_calls += 1;
        if state.fail_clear {
            return Err(Error::new(0, "合成设备清理失败"));
        }
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("clear_touches");
        Ok(())
    }
    fn cancel_wheel_repeat(&mut self) -> Result<()> {
        let mut state = self.0.borrow_mut();
        state.calls.push("cancel_wheel_repeat");
        if state.fail_wheel_cancel {
            return Err(Error::new(0, "合成滚轮取消失败"));
        }
        state.wheel_repeats = 0;
        Ok(())
    }
}

fn input_command(runtime: &mut TestRuntime, code: u32, data: [u32; 3]) -> Result<u32> {
    runtime.dispatch_command(
        0,
        Command {
            sender: 0,
            target: 1,
            code,
            data,
        },
    )
}

fn integers(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn device_update_preserves_query_times_and_continues_after_key_consumption() -> Result<()> {
    let (mut runtime, _, records) = runtime(8);
    register(&mut runtime, 2, 0, [0; 4]);
    let state = Rc::new(RefCell::new(DeviceState::new([10, 20])));
    state.borrow_mut().positions = [[10, 20], [10, 20], [30, 40], [50, 60]].into();
    state.borrow_mut().key_mask = 2;
    runtime.input_coordinates_mut().device_scale = 2.;
    runtime.set_input_device(Device(state.clone()))?;
    assert_eq!(runtime.input_state(1).unwrap().pointer().position, [10, 20]);
    assert!(runtime.update(1, 1)?);
    let records = records.borrow();
    assert_eq!(
        records.iter().map(|r| r.packet.code).collect::<Vec<_>>(),
        [4, 6]
    );
    assert_eq!(integers(&records[0].points), [25, 30]);
    assert_eq!(integers(&records[1].points), [15, 20, 5, 10]);
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.pointer().position, [10, 20]);
    assert_eq!(input.pointer().origin, [10, 20]);
    assert_eq!(input.events().suppressed(), 1);
    assert_eq!(input.events().captures()[0], 2);
    assert_eq!(state.borrow().pointer_calls, 4);
    assert_eq!(
        state.borrow().queries,
        [(0, 0, true), (0, 0, false), (0, 1, false)]
    );
    assert!(
        state
            .borrow()
            .calls
            .ends_with(&["finish", "commit", "history"])
    );

    state.borrow_mut().sample.position = [14, -18];
    let mut x = [0xaa; 8];
    let mut y = [0xbb; 8];
    runtime.dispatch_command_with_buffers(
        0,
        Command {
            sender: 0,
            target: 1,
            code: 126,
            data: [0; 3],
        },
        &mut CommandBuffers {
            data0: Some(&mut x),
            data1: Some(&mut y),
            external: None,
        },
    )?;
    assert_eq!(integers(&x[..4]), [7]);
    assert_eq!(integers(&y[..4]), [-9]);
    assert_eq!(&x[4..], &[0xaa; 4]);
    assert_eq!(&y[4..], &[0xbb; 4]);
    let id = runtime.create(0, kind::INPUT, b"second-input", b"")?;
    assert_eq!(
        runtime.input_state(id).unwrap().pointer().position,
        [14, -18]
    );
    assert_eq!(state.borrow().pointer_calls, 6);
    Ok(())
}

#[test]
fn automatic_cursor_broadcast_exposes_live_state_and_keeps_callback_changes() -> Result<()> {
    let (mut runtime, _, records) = runtime(7);
    let state = Rc::new(RefCell::new(DeviceState::new([0; 2])));
    runtime.set_input_device(Device(state.clone()))?;
    let pointer = runtime.input_state_mut(1).unwrap().pointer_mut();
    pointer.auto_hide = 1;
    pointer.idle_limit = 2;
    assert!(runtime.update(1, 1)?);
    assert!(records.borrow().is_empty());
    assert!(runtime.update(2, 1)?);
    assert_eq!(
        runtime.input_state(1).unwrap().pointer().cursor_requested,
        0
    );
    assert_eq!(runtime.input_state(1).unwrap().pointer().cursor_shown, 0);
    state.borrow_mut().sample.position = [1; 2];
    assert!(runtime.update(3, 1)?);
    assert_eq!(state.borrow().shows, [0, 0]);
    input_command(&mut runtime, 150, [0, 1, 0])?;
    input_command(&mut runtime, 146, [0; 3])?;
    assert!(runtime.update(4, 1)?);
    assert_eq!(runtime.input_state(1).unwrap().pointer().cursor_shown, 0);
    state.borrow_mut().sample.position = [2; 2];
    assert!(runtime.update(5, 1)?);
    assert_eq!(state.borrow().shows, [0, 0, 1, 1]);
    let records = records.borrow();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.packet.target, r.packet.data))
            .collect::<Vec<_>>(),
        [
            (2, [0, 0, 0]),
            (3, [0, 0, 0]),
            (2, [0, 1, 0]),
            (3, [0, 1, 0])
        ]
    );
    assert!(records.iter().all(|r| r.packet.sender == 1
        && r.packet.code == 22
        && integers(&r.points) == [r.packet.data[1] as i32]));
    Ok(())
}

#[test]
fn wheel_ignores_event_masks_and_snapshot_commands_preserve_buffer_tails() -> Result<()> {
    let (mut runtime, _, records) = runtime(0);
    register(&mut runtime, 2, 0, [0, 0, 8, 8]);
    input_command(&mut runtime, 132, [2, 0, 0])?;
    input_command(&mut runtime, 136, [2, 0, 0])?;
    input_command(&mut runtime, 154, [2, 0, 0])?;
    runtime
        .input_state_mut(1)
        .unwrap()
        .regions_mut()
        .set_hit_mask(2, Some(HitMask::new([8, 8], vec![0; 8])?));
    let state = Rc::new(RefCell::new(DeviceState::new([4; 2])));
    state.borrow_mut().wheel = 1;
    runtime.set_input_device(Device(state.clone()))?;
    assert!(runtime.update(1, 1)?);
    assert!(records.borrow().is_empty());
    runtime
        .input_state_mut(1)
        .unwrap()
        .regions_mut()
        .set_hit_mask(2, None);
    state.borrow_mut().wheel = u32::MAX;
    assert!(runtime.update(2, 1)?);
    let records = records.borrow();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].packet,
        Command {
            sender: 1,
            target: 2,
            code: 5,
            data: [0; 3]
        }
    );
    let mut history = [0xaa; 36];
    runtime.dispatch_command_with_buffers(
        0,
        Command {
            sender: 0,
            target: 1,
            code: 160,
            data: [0; 3],
        },
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut history),
            external: None,
        },
    )?;
    assert_eq!(
        &history[..32],
        &std::array::from_fn::<_, 32, _>(|i| i as u8)
    );
    assert_eq!(&history[32..], &[0xaa; 4]);
    let cached = runtime.input_state(1).unwrap().pointer().clone();
    let calls = state.borrow().calls.len();
    input_command(&mut runtime, 156, [0x1234, 0x5678, 0])?;
    assert_eq!(&state.borrow().calls[calls..], &["clear_touches"]);
    assert_eq!(runtime.input_state(1).unwrap().pointer(), &cached);
    let keys = *runtime.input_state(1).unwrap().keys().records();
    let captures = *runtime.input_state(1).unwrap().events().captures();
    state.borrow_mut().wheel_repeats = 9;
    state.borrow_mut().fail_wheel_cancel = true;
    assert!(input_command(&mut runtime, 158, [u32::MAX; 3]).is_err());
    assert_eq!(state.borrow().wheel_repeats, 9);
    state.borrow_mut().fail_wheel_cancel = false;
    input_command(&mut runtime, 158, [u32::MAX; 3])?;
    assert_eq!(state.borrow().wheel_repeats, 0);
    assert_eq!(runtime.input_state(1).unwrap().pointer(), &cached);
    assert_eq!(runtime.input_state(1).unwrap().keys().records(), &keys);
    assert_eq!(
        runtime.input_state(1).unwrap().events().captures(),
        &captures
    );
    input_command(&mut runtime, 160, [0; 3])?;
    assert_eq!(input_command(&mut runtime, 153, [0; 3])?, 0);
    state.borrow_mut().sample.active = 0;
    assert!(runtime.update(3, 1)?);
    assert_eq!(input_command(&mut runtime, 153, [0; 3])?, 1);
    input_command(&mut runtime, 3, [0; 3])?;
    input_command(&mut runtime, 3, [0; 3])?;
    assert_eq!(state.borrow().shows, [1]);
    Ok(())
}

#[test]
fn failed_press_position_query_retains_key_state_and_restores_object_access() -> Result<()> {
    let (mut runtime, _, _) = runtime(0);
    register(&mut runtime, 2, 0, [0; 4]);
    let state = Rc::new(RefCell::new(DeviceState::new([3; 2])));
    state.borrow_mut().key_mask = 2;
    state.borrow_mut().fail_pointer = Some(3);
    runtime.set_input_device(Device(state.clone()))?;
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("合成设备查询失败")
    );
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.keys().records()[0][0].count, 1);
    assert_eq!(input.keys().records()[0][0].mask, 2);
    assert_eq!(input.regions().records().len(), 1);
    assert!(runtime.object(1).is_some());
    assert_eq!(input_command(&mut runtime, 134, [2, 0, 0])?, 0);
    Ok(())
}

#[test]
fn cursor_commands_keep_nested_changes_and_restore_state_after_callback_errors() -> Result<()> {
    let (mut runtime, mode, records) = runtime(9);
    let state = Rc::new(RefCell::new(DeviceState::new([0; 2])));
    runtime.set_input_device(Device(state.clone()))?;
    let input = runtime.input_state_mut(1).unwrap();
    input.set_update_rate(120);
    input.pointer_mut().idle_count = 9;
    input_command(&mut runtime, 143, [5, u32::MAX, 0])?;
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.update_rate(), 120);
    assert_eq!(input.pointer().auto_hide, 5);
    assert_eq!(input.pointer().idle_limit, 71_582_786);
    assert_eq!(input.pointer().idle_count, 0);
    assert_eq!(input.pointer().cursor_shown, 1);
    assert_eq!(input.pointer().cursor_requested, 0);
    assert_eq!(state.borrow().shows, [1, 0]);
    assert_eq!(
        records
            .borrow()
            .iter()
            .map(|r| (r.packet.code, r.packet.target))
            .collect::<Vec<_>>(),
        [(22, 2), (22, 3)]
    );

    runtime.input_state_mut(1).unwrap().pointer_mut().idle_count = 7;
    input_command(&mut runtime, 143, [0, 1, 0])?;
    assert_eq!(runtime.input_state(1).unwrap().pointer().idle_count, 7);
    input_command(&mut runtime, 159, [0; 3])?;
    assert_eq!(runtime.input_state(1).unwrap().pointer().idle_count, 0);
    assert_eq!(state.borrow().shows, [1, 0]);

    mode.set(11);
    input_command(&mut runtime, 150, [0, 1, 0])?;
    runtime.input_state_mut(1).unwrap().pointer_mut().idle_count = 11;
    assert!(
        input_command(&mut runtime, 143, [7, 1, 0])
            .unwrap_err()
            .message
            .contains("155")
    );
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.pointer().auto_hide, 7);
    assert_eq!(input.pointer().idle_limit, 2);
    assert_eq!(input.pointer().idle_count, 11);
    assert_eq!(input.pointer().cursor_shown, 1);
    assert!(runtime.object(1).is_some());
    mode.set(0);
    input_command(&mut runtime, 159, [0; 3])?;
    assert_eq!(runtime.input_state(1).unwrap().pointer().idle_count, 0);
    Ok(())
}

#[test]
fn positioning_gates_and_cleanup_preserve_captures_disabled_keys_and_partial_errors() -> Result<()>
{
    let (mut runtime, mode, _) = runtime(6);
    register(&mut runtime, 2, 0, [0; 4]);
    runtime.scan_input_keys(1, [0; 2], |key, group| {
        Ok(KeySample {
            mask: if key < 2 && group == 0 { 2 } else { 0 },
            repeat: 1,
        })
    })?;
    mode.set(0);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    input_command(&mut runtime, 151, [1, 0, 0])?;
    let state = Rc::new(RefCell::new(DeviceState::new([0; 2])));
    runtime.set_input_device(Device(state.clone()))?;
    runtime.input_coordinates_mut().logical_size = [640, 480];
    runtime.input_coordinates_mut().device_scale = 2.;
    runtime.input_coordinates_mut().configuration_offset = [1, -1];
    let clock = runtime.object_mut(1).unwrap().state().1;
    clock.time = 100;
    clock.origin = 10;
    let input = runtime.input_state_mut(1).unwrap();
    input.events_mut().set_suppressed(7);
    input.pointer_mut().move_target = [-8, -9];
    input.pointer_mut().move_remaining = 99;
    let packed = 10 | 20 << 16;
    input_command(&mut runtime, 138, [5, packed, 0])?;
    assert_eq!(state.borrow().calls, ["pointer"]);
    state.borrow_mut().foreground = false;
    input_command(&mut runtime, 139, [5, packed, 0])?;
    state.borrow_mut().foreground = true;
    input_command(&mut runtime, 139, [5, 640 | 20 << 16, 0])?;
    assert_eq!(
        runtime.input_state(1).unwrap().pointer().move_target,
        [-8, -9]
    );
    assert_eq!(runtime.input_state(1).unwrap().events().suppressed(), 7);
    assert_eq!(state.borrow().clear_calls, 0);

    state.borrow_mut().fail_clear = true;
    assert!(
        input_command(&mut runtime, 139, [5, packed, 0])
            .unwrap_err()
            .message
            .contains("合成设备清理失败")
    );
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.events().suppressed(), 0);
    assert_eq!(input.events().captures()[0], 2);
    assert_eq!(input.events().raw_state(), 2);
    assert_eq!(input.keys().records()[0][0], Default::default());
    assert_eq!(input.keys().records()[1][0].count, 1);
    assert_eq!(input.keys().records()[1][0].mask, 2);
    assert_eq!(input.pointer().move_target, [-8, -9]);
    assert_eq!(input.pointer().move_remaining, 99);

    state.borrow_mut().fail_clear = false;
    runtime.set_input_pointer_control(true);
    runtime
        .input_state_mut(1)
        .unwrap()
        .events_mut()
        .set_suppressed(1);
    input_command(&mut runtime, 138, [u32::MAX, packed, 0])?;
    let pointer = runtime.input_state(1).unwrap().pointer();
    assert_eq!(pointer.move_target, [22, 38]);
    assert_eq!(pointer.move_remaining, -1);
    assert_eq!(pointer.move_started, 90);
    assert_eq!(state.borrow().clear_calls, 2);
    runtime.input_coordinates_mut().logical_size = [65_536; 2];
    input_command(&mut runtime, 139, [0, u32::MAX, 0])?;
    assert_eq!(
        runtime.input_state(1).unwrap().pointer().move_target,
        [131_072, 131_068]
    );
    assert_eq!(runtime.input_state(1).unwrap().pointer().move_remaining, 0);
    assert_eq!(state.borrow().clear_calls, 2);
    assert!(state.borrow().moves.is_empty());
    Ok(())
}

#[test]
fn positioning_keeps_global_time_while_paused_and_the_baseline_saved_point() -> Result<()> {
    let (mut runtime, _, records) = runtime(0);
    register(&mut runtime, 2, 0, [18, 28, 23, 33]);
    let state = Rc::new(RefCell::new(DeviceState::new([10, 20])));
    runtime.set_input_device(Device(state.clone()))?;
    runtime.input_coordinates_mut().logical_size = [1000; 2];
    let clock = runtime.object_mut(1).unwrap().state().1;
    clock.time = 100;
    clock.previous_time = 100;
    clock.origin = 10;
    clock.paused = 1;
    input_command(&mut runtime, 139, [10, 110 | 120 << 16, 0])?;
    let pointer = runtime.input_state_mut(1).unwrap().pointer_mut();
    pointer.auto_hide = 1;
    pointer.cursor_shown = 0;
    pointer.idle_count = 55;
    pointer.forced = 1;
    assert!(runtime.update(101, 999)?);
    let pointer = runtime.input_state(1).unwrap().pointer();
    assert_eq!(pointer.move_remaining, 1);
    assert_eq!(pointer.position, [30, 20]);
    assert_eq!(pointer.idle_count, 0);
    assert_eq!(pointer.forced, 1);
    assert_eq!(state.borrow().moves, [[20, 30]]);
    assert_eq!(
        records
            .borrow()
            .iter()
            .map(|r| (r.packet.code, r.packet.target, r.packet.data))
            .collect::<Vec<_>>(),
        [(7, 2, [20, 30, 0]), (22, 2, [0, 1, 0]), (22, 3, [0, 1, 0])]
    );
    assert!(runtime.update(102, 1)?);
    assert_eq!(state.borrow().moves, [[20, 30], [110, 120]]);
    assert_eq!(runtime.input_state(1).unwrap().pointer().move_remaining, 0);
    assert_eq!(
        runtime.input_state(1).unwrap().pointer().position,
        [120, 20]
    );
    assert_eq!(records.borrow().last().unwrap().packet.code, 8);
    assert_eq!(runtime.object(1).unwrap().clock().time, 100);

    state.borrow_mut().sample.position = [0; 2];
    let pointer = runtime.input_state_mut(1).unwrap().pointer_mut();
    pointer.move_target = [5, 6];
    pointer.move_remaining = 1;
    pointer.cursor_shown = 0;
    pointer.idle_count = 9;
    assert!(runtime.update(103, 1)?);
    assert_eq!(state.borrow().moves.last(), Some(&[5, 6]));
    let pointer = runtime.input_state(1).unwrap().pointer();
    assert_eq!(pointer.position, [6, 0]);
    assert_eq!(pointer.cursor_shown, 0);
    assert_eq!(pointer.idle_count, 0);
    assert_eq!(state.borrow().shows, [1]);
    Ok(())
}

#[test]
fn positioning_in_motion_callbacks_clears_suppression_even_after_device_errors() -> Result<()> {
    let (mut runtime, mode, records) = runtime(6);
    register(&mut runtime, 2, 1, [0; 4]);
    register(&mut runtime, 3, 0, [0; 4]);
    runtime.scan_input_keys(1, [0; 2], |key, group| {
        Ok(KeySample {
            mask: if key == 0 && group == 0 { 2 } else { 0 },
            repeat: 1,
        })
    })?;
    mode.set(0);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    let state = Rc::new(RefCell::new(DeviceState::new([0; 2])));
    runtime.set_input_device(Device(state.clone()))?;
    runtime.input_coordinates_mut().logical_size = [100; 2];
    mode.set(12);
    records.borrow_mut().clear();
    state.borrow_mut().fail_clear = true;
    runtime
        .input_state_mut(1)
        .unwrap()
        .events_mut()
        .set_suppressed(1);
    let motion = PointerMotion {
        code: 6,
        previous: [0; 2],
        current: [10; 2],
    };
    assert!(
        runtime
            .dispatch_input_motion(1, motion)
            .unwrap_err()
            .message
            .contains("合成设备清理失败")
    );
    assert_eq!(runtime.input_state(1).unwrap().events().suppressed(), 0);
    assert_eq!(
        runtime.input_state(1).unwrap().keys().records()[0][0],
        Default::default()
    );
    assert_eq!(records.borrow().len(), 1);
    state.borrow_mut().fail_clear = false;
    runtime
        .input_state_mut(1)
        .unwrap()
        .events_mut()
        .set_suppressed(1);
    runtime.dispatch_input_motion(1, motion)?;
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.events().suppressed(), 0);
    assert_eq!(input.events().captures()[0], 2);
    assert_eq!(input.events().raw_state(), 2);
    assert_eq!(input.pointer().move_target, [50, 60]);
    assert_eq!(
        records
            .borrow()
            .iter()
            .map(|r| r.packet.target)
            .collect::<Vec<_>>(),
        [2, 2, 3]
    );
    assert!(runtime.dispatch_input_key(1, event(2))?);
    assert_eq!(state.borrow().clear_calls, 2);
    Ok(())
}
