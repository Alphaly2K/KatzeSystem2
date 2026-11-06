use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

fn ins(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, arguments: &[(u8, u32)]) {
    for &argument in arguments {
        ins(code, 0x25, &[argument]);
    }
    ins(code, 0x2b, &[(3, index), (3, arguments.len() as u32 * 4)]);
}

fn store(code: &mut Vec<u8>, address: u32) {
    ins(code, 0x26, &[(3, address)]);
}

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn word(vm: &katze_system2::kcs::Interpreter, address: u32) -> u32 {
    u32::from_le_bytes(vm.read(address, 4).unwrap().try_into().unwrap())
}

#[test]
fn ordinary_motion_properties_preserve_groups_save_direction_and_handle_lifetime() {
    let mut data = vec![0; 2048];
    let properties: [u32; 36] = std::array::from_fn(|i| 0x8000_0100 + i as u32);
    for (i, value) in properties.iter().enumerate() {
        data[128 + i * 4..132 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0xdead_beef)]);
    call(&mut code, 294, &[(3, u32::MAX)]);
    store(&mut code, 1900);
    call(&mut code, 586, &[(0x13, 1900), (3, 0x8000_1234)]);
    call(&mut code, 586, &[(3, 0)]);
    call(&mut code, 112, &[(3, 0), (3, 0)]);
    store(&mut code, 1904);

    let offsets = [0, 12, 24, 36, 100, 112, 116, 128, 140];
    let mut inputs = vec![(0x13, 1900)];
    inputs.extend(offsets.map(|offset| (3, 128 + offset)));
    inputs[6] = (3, properties[28]);
    inputs[9] = (3, properties[35]);
    call(&mut code, 301, &inputs);
    call(&mut code, 303, &[(0x13, 1900)]);

    let mut outputs = vec![(0x13, 1900)];
    outputs.extend(offsets.map(|offset| (3, 512 + offset)));
    call(&mut code, 300, &outputs);
    call(&mut code, 307, &[(0x13, 1900), (3, 99), (3, 1)]);
    store(&mut code, 1908);
    for (index, address, arguments) in [
        (309, 1912, vec![(0x13, 1900), (3, 999)]),
        (310, 1916, vec![(0x13, 1900), (3, u32::MAX)]),
        (311, 1920, vec![(0x13, 1900)]),
    ] {
        call(&mut code, index, &arguments);
        store(&mut code, address);
    }
    let mut overlap = vec![(0x13, 1900)];
    overlap.extend([(3, 768); 9]);
    call(&mut code, 300, &overlap);
    let mut optional = vec![(0x13, 1900)];
    optional.extend(offsets.map(|offset| (3, 1024 + offset)));
    optional[9] = (3, u32::MAX);
    call(&mut code, 302, &optional);
    call(&mut code, 295, &[(0x13, 1900)]);
    call(&mut code, 294, &[]);
    store(&mut code, 1924);
    for index in (295..=314).chain([535]) {
        call(&mut code, index, &[(3, 0)]);
        if matches!(index, 304 | 307 | 309..=311 | 535) {
            ins(&mut code, 0x2d, &[]);
        }
    }
    code.push(0x3b);
    let program = Program::from_code(&code, data, 128).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"motion", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let read =
            |at: u32, count: u32| (0..count).map(|i| word(vm, at + i * 4)).collect::<Vec<_>>();
        assert_eq!(read(512, 36), properties);
        assert_eq!(read(1024, 35), properties[..35]);
        assert_eq!(word(vm, 1164), 0);
        assert_eq!(word(vm, 768), properties[35]);
        assert_eq!(read(772, 2), properties[33..35]);
        assert_eq!(read(780, 13), properties[12..25]);
        assert_eq!(read(1908, 4), [0; 4]);
        assert_eq!(word(vm, 1900), 1);
        assert_eq!(word(vm, 1904), 2);
        assert!(word(vm, 1924) > word(vm, 1904));
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}

#[test]
fn motion_loading_copies_parameters_and_saved_data_and_reuses_programs() {
    use katze_system2::{kcs::ResourceServices, resource::ResourceMode};
    use std::{cell::RefCell, io::Write, rc::Rc};

    struct Files(Vec<u8>, Rc<RefCell<Vec<Vec<u8>>>>);
    impl ResourceServices for Files {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"mot");
            Ok(name.to_vec())
        }
        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            self.1.borrow_mut().push(name.to_vec());
            Ok((name != b"missing").then(|| self.0.clone()))
        }
    }

    let mut movement = Vec::new();
    ins(&mut movement, 0x25, &[(0x13, 0x8000_049c)]);
    let branch = movement.len();
    ins(&mut movement, 0x27, &[(3, 0)]);
    ins(&mut movement, 0x25, &[(0x13, 0x8000_049c)]);
    ins(&mut movement, 0x15, &[(0x43, 0), (3, 0x1122_3344)]);
    movement.push(0x2d);
    let end = movement.len() as u32;
    movement[branch + 2..branch + 6].copy_from_slice(&end.to_le_bytes());
    movement.push(0x3b);
    let mut blob = Vec::new();
    for value in [
        0x0053_434b,
        2,
        0,
        52,
        movement.len() as u32,
        52 + movement.len() as u32,
        0,
        0,
        16,
        64,
        52 + movement.len() as u32,
        0,
        movement.len() as u32,
    ] {
        blob.extend(value.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&movement).unwrap();
    blob.extend(encoder.finish().unwrap());

    let mut data = vec![0; 1600];
    data[48..52].copy_from_slice(&7u32.to_le_bytes());
    data[64..81].copy_from_slice(b"path/move%d[G48]\0");
    data[96..107].copy_from_slice(b"PATH/MOVE7\0");
    let parameters = std::array::from_fn::<_, 256, _>(|i| 0x1122_0000 + i as u32);
    for (bytes, value) in data[128..1152].chunks_exact_mut(4).zip(parameters) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    data[1152..1160].copy_from_slice(b"missing\0");
    data[1200..1208].copy_from_slice(b"abcdefgh");
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0xdead_beef)]);
    call(&mut code, 294, &[]);
    store(&mut code, 0);
    call(&mut code, 586, &[(3, 1), (3, 0x1234_5678)]);
    call(
        &mut code,
        535,
        &[
            (3, 1),
            (3, 1152),
            (3, u32::MAX),
            (3, 1),
            (3, u32::MAX),
            (3, 8),
            (3, 0),
            (3, 0),
        ],
    );
    store(&mut code, 4);
    call(
        &mut code,
        535,
        &[
            (3, 1),
            (3, 64),
            (3, 128),
            (3, 257),
            (3, 1200),
            (3, 8),
            (3, (-3i32) as u32),
            (3, 0x8000_0000),
        ],
    );
    store(&mut code, 8);
    call(
        &mut code,
        304,
        &[
            (3, 1),
            (3, 96),
            (3, u32::MAX),
            (3, 0),
            (3, u32::MAX),
            (3, 8),
            (3, 7),
        ],
    );
    store(&mut code, 12);
    call(&mut code, 65, &[]);
    call(&mut code, 36, &[(3, 1200), (3, 0xff), (3, 8)]);
    call(&mut code, 307, &[(3, 1), (3, 100), (3, 0)]);
    store(&mut code, 16);
    call(&mut code, 315, &[(3, 1), (3, 100)]);
    store(&mut code, 20);
    call(&mut code, 65, &[]);
    call(&mut code, 295, &[(3, 1)]);
    call(&mut code, 317, &[(3, 2)]);
    code.push(0x3b);
    let program = Program::from_code(&code, data, 128).unwrap();

    for budget in [1, 4096] {
        let reads = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_resources(Files(blob.clone(), reads.clone()));
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"motion", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let queue = runtime.motion_controller(1).unwrap();
        assert_eq!(
            (queue.len(), queue.group(1), queue.group(2)),
            (2, Some(-3), Some(7))
        );
        let first = queue.task(1).unwrap();
        assert_eq!(first.state().unwrap().parameters, parameters);
        assert_eq!(
            first.snapshot_name,
            "move7".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(first.snapshot_flags, 0x8000_0000);
        assert_eq!(first.snapshot_data, Some((80, 8)));
        assert_eq!(first.interpreter.read(80, 8).unwrap(), b"abcdefgh");
        assert_eq!(word(&first.interpreter, 0x8000_049c), 80);
        assert_eq!(word(&first.interpreter, 0x8000_04a0), 8);
        assert_eq!(word(&first.interpreter, 0x8000_04a4), 0x1234_5678);
        let second = queue.task(2).unwrap();
        assert_eq!(second.state().unwrap().parameters, [0; 256]);
        assert_eq!(second.snapshot_flags, 0);
        assert_eq!(second.snapshot_data, None);
        assert_eq!(
            *reads.borrow(),
            [b"missing".to_vec(), b"path/move7".to_vec()]
        );

        runtime.update(1, 1).unwrap();
        let bytes = runtime.motion_snapshot(2).unwrap().bytes();
        assert_eq!(bytes.len(), 8 + 0x454 * 2 + 8);
        assert_eq!(&bytes[8 + 0x454..8 + 0x45c], b"\x44\x33\x22\x11efgh");
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            (0..6).map(|i| word(vm, i * 4)).collect::<Vec<_>>(),
            [1, 0, 1, 2, 1, 2]
        );
        assert_eq!(vm.read(1200, 8).unwrap(), [0xff; 8]);
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
        runtime.update(2, 1).unwrap();
        assert!(runtime.motion_controller(1).is_none());
        assert!(runtime.motion_snapshot(2).is_none());
    }
}

#[test]
fn native_finish_runs_every_task_and_preserves_controls_and_queue_entries() {
    use katze_system2::kcs::{MotionState, MotionTask};

    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0xdead_beef)]);
    call(&mut code, 440, &[(3, 0)]);
    store(&mut code, 8);
    call(&mut code, 294, &[]);
    store(&mut code, 0);
    call(&mut code, 440, &[(0x13, 0)]);
    store(&mut code, 12);
    call(&mut code, 65, &[]);
    for address in [16, 20] {
        call(&mut code, 440, &[(0x13, 0)]);
        store(&mut code, address);
    }
    call(&mut code, 65, &[]);
    call(&mut code, 296, &[(0x13, 0)]);
    call(&mut code, 440, &[(0x13, 0)]);
    store(&mut code, 24);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 32], 128).unwrap();
    let mut motion = Vec::new();
    ins(&mut motion, 0x16, &[(3, 0)]);
    motion.push(0x2d);
    ins(
        &mut motion,
        1,
        &[(0x14, 0x8000_0000), (4, 1.5f32.to_bits())],
    );
    for (address, flags, value) in [
        (0x8000_0000, 0x44, 0),
        (0x8000_0004, 0x13, 0x8000_0090),
        (0x8000_0008, 0x13, 0x8000_0098),
    ] {
        ins(&mut motion, 0x15, &[(3, address), (flags, value)]);
        motion.push(0x2d);
    }
    motion.push(0x3b);
    let motion = Program::from_code(&motion, vec![0; 16], 128).unwrap();
    for budget in [1, 4096] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        rt.set_budget(budget, 1 << 20);
        let caller = rt.create(0, kind::ORDINARY, b"finish", b"").unwrap();
        rt.update(0, 1).unwrap();
        let queue = rt.motion_controller_mut(1).unwrap();
        queue.base_properties[0] = 2.0f32.to_bits();
        for _ in 0..2 {
            queue
                .insert(
                    MotionTask::new(
                        motion.clone(),
                        MotionState {
                            elapsed: 99,
                            running: -2,
                            finishing: 7,
                            ..Default::default()
                        },
                        u32::MAX,
                    )
                    .unwrap(),
                    0,
                )
                .unwrap();
        }
        rt.update(1, 1).unwrap();
        let queue = rt.motion_controller(1).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(&queue.properties()[..3], &[5.0f32.to_bits(), u32::MAX, 1]);
        for id in [1, 2] {
            let task = queue.task(id).unwrap();
            let state = task.state().unwrap();
            assert_eq!((state.elapsed, state.running, state.finishing), (99, -2, 7));
            assert_eq!(word(&task.interpreter, 0), 2);
        }
        rt.update(2, 1).unwrap();
        let queue = rt.motion_controller(1).unwrap();
        assert!(queue.is_empty());
        assert_eq!(queue.properties(), &queue.base_properties);
        let vm = rt.object(caller).unwrap().interpreter().unwrap();
        for (address, expected) in [(8, 0), (12, 0), (16, 1), (20, 1), (24, 0)] {
            assert_eq!(word(vm, address), expected);
        }
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}

#[test]
fn snapshots_recapture_the_original_controller_and_keep_capacity_until_release() {
    use katze_system2::kcs::{MotionState, MotionTask};

    let mut code = vec![];
    ins(&mut code, 0x25, &[(3, 0xdead_beef)]);
    call(&mut code, 294, &[]);
    store(&mut code, 0);
    call(&mut code, 315, &[(3, 0)]);
    store(&mut code, 4);
    call(&mut code, 316, &[(3, 0)]);
    store(&mut code, 8);
    call(&mut code, 317, &[(3, 0)]);
    call(&mut code, 315, &[(0x13, 0)]);
    store(&mut code, 28);
    call(&mut code, 65, &[]);
    call(&mut code, 315, &[(0x13, 0), (3, 5)]);
    store(&mut code, 12);
    call(&mut code, 112, &[(3, 0), (3, 0)]);
    store(&mut code, 24);
    call(&mut code, 65, &[]);
    for (time, output) in [(9, 16), (11, 20)] {
        if time == 11 {
            call(&mut code, 316, &[(0x13, 12)]);
        } else {
            call(&mut code, 316, &[(0x13, 12), (3, time)]);
        }
        store(&mut code, output);
        call(&mut code, 65, &[]);
    }
    call(&mut code, 295, &[(0x13, 0)]);
    call(&mut code, 65, &[]);
    call(&mut code, 317, &[(0x13, 12)]);
    call(&mut code, 317, &[(0x13, 28)]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 32], 64).unwrap();
    let motion_program = Program::from_code(&[0x3b], vec![], 0).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime
            .create(0, kind::ORDINARY, b"snapshots", b"")
            .unwrap();
        runtime.update(0, 1).unwrap();
        assert_eq!(
            runtime.motion_snapshot(2).unwrap().bytes(),
            &[8, 0, 0, 0, 0, 0, 0, 0]
        );
        let mut motion = MotionTask::new(
            motion_program.clone(),
            MotionState {
                parameters: std::array::from_fn(|i| 0x8000_0000 + i as u32),
                ..MotionState::default()
            },
            u32::MAX,
        )
        .unwrap();
        motion.snapshot_name = "move\0ignored".encode_utf16().collect();
        motion.snapshot_flags = 0x1234_5678;
        motion.snapshot_data = Some((0x8000_0020, 3));
        motion.interpreter.write(0x8000_0020, b"abc").unwrap();
        let first = runtime
            .motion_controller_mut(1)
            .unwrap()
            .insert(motion, -3)
            .unwrap();
        runtime.update(1, 1).unwrap();
        let snapshot = runtime.motion_snapshot(3).unwrap();
        let bytes = snapshot.bytes();
        let read = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(snapshot.controller(), 1);
        assert_eq!(snapshot.capacity(), 8 + 0x454 + 3);
        assert_eq!(read(0), snapshot.capacity() as u32);
        assert_eq!(read(4), 1);
        assert_eq!(&bytes[12..22], b"m\0o\0v\0e\0\0\0");
        assert_eq!(read(8 + 0x44), (-3i32) as u32);
        assert_eq!(read(8 + 0x48), 0x8000_0000);
        assert_eq!(read(8 + 0x444), 0x8000_00ff);
        assert_eq!(read(8 + 0x448), 3);
        assert_eq!(read(8 + 0x44c), 6);
        assert_eq!(read(8 + 0x450), 0x1234_5678);
        assert_eq!(&bytes[8 + 0x454..], b"abc");
        let queue = runtime.motion_controller_mut(1).unwrap();
        queue.set_parameters(first, &[0x1122_3344]).unwrap();
        queue
            .insert(
                MotionTask::new(motion_program.clone(), MotionState::default(), 1).unwrap(),
                4,
            )
            .unwrap();
        runtime.update(2, 1).unwrap();
        let snapshot = runtime.motion_snapshot(3).unwrap();
        let capacity = 8 + 0x454 * 2 + 3;
        assert_eq!(snapshot.capacity(), capacity);
        assert_eq!(
            &snapshot.bytes()[8 + 0x48..8 + 0x4c],
            &0x1122_3344u32.to_le_bytes()
        );
        runtime.motion_controller_mut(1).unwrap().clear();
        runtime.update(3, 1).unwrap();
        let snapshot = runtime.motion_snapshot(3).unwrap();
        assert_eq!(snapshot.capacity(), capacity);
        assert_eq!(snapshot.bytes(), &[8, 0, 0, 0, 0, 0, 0, 0]);
        runtime.update(4, 1).unwrap();
        assert!(runtime.motion_controller(1).is_none());
        assert!(runtime.motion_snapshot(3).is_some());
        runtime.update(5, 1).unwrap();
        assert!(runtime.motion_snapshot(2).is_none());
        assert!(runtime.motion_snapshot(3).is_none());
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            [
                word(vm, 4),
                word(vm, 8),
                word(vm, 12),
                word(vm, 16),
                word(vm, 20),
                word(vm, 24)
            ],
            [0, 0, 3, 1, 1, 4]
        );
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}
