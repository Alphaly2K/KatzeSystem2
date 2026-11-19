use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, args: &[(u8, u32)]) {
    for &arg in args {
        instruction(code, 0x25, &[arg]);
    }
    instruction(code, 0x2b, &[(3, index), (3, args.len() as u32 * 4)]);
}

#[test]
fn legacy_cst_registrations_are_owned_by_each_task_until_reset_or_release() {
    let mut code = vec![];
    call(&mut code, 227, &[(3, 64)]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    call(&mut code, 65, &[]);
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 219, &[]);
    for (index, name, flag, value) in [
        (223, 64, 0x13, 4),
        (223, 80, 3, 5),
        (224, 64, 3, 6),
        (224, 96, 3, 0),
    ] {
        call(&mut code, index, &[(3, name), (flag, value)]);
        instruction(&mut code, 0x26, &[(3, 12)]);
    }
    call(&mut code, 226, &[(3, u32::MAX)]);
    call(&mut code, 227, &[(3, 80)]);
    instruction(&mut code, 0x26, &[(3, 16)]);
    call(&mut code, 227, &[(3, 112)]);
    instruction(&mut code, 0x26, &[(3, 20)]);
    call(&mut code, 225, &[]);
    call(&mut code, 227, &[(3, 64)]);
    instruction(&mut code, 0x26, &[(3, 24)]);
    call(&mut code, 223, &[(3, 64), (3, 19)]);
    instruction(&mut code, 0x26, &[(3, 28)]);
    instruction(&mut code, 0x26, &[(3, 32)]);
    call(&mut code, 65, &[]);
    call(&mut code, 219, &[]);
    call(&mut code, 227, &[(3, 64)]);
    instruction(&mut code, 0x26, &[(3, 36)]);
    code.push(0x3b);
    let mut data = vec![0; 128];
    data[64..74].copy_from_slice(b"scene.CST\0");
    data[80..90].copy_from_slice(b"SCENE.cst\0");
    data[112..118].copy_from_slice(b"scene\0");
    let program = Program::from_code(&code, data, 64).unwrap();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(program)),
    );
    let a = runtime.create(0, kind::ORDINARY, b"a", b"").unwrap();
    let b = runtime.create(0, kind::ORDINARY, b"b", b"").unwrap();
    for (task, value) in [(a, 7u32), (b, u32::MAX)] {
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(4, &value.to_le_bytes())
            .unwrap();
    }
    let word = |game: &Runtime<KcsFactory<Loader>>, task, addr| {
        let vm = game.object(task).unwrap().interpreter().unwrap();
        u32::from_le_bytes(vm.read(addr, 4).unwrap().try_into().unwrap())
    };
    runtime.update(0, 1).unwrap();
    runtime.update(1, 1).unwrap();
    for (task, value) in [(a, 7), (b, u32::MAX)] {
        assert_eq!(word(&runtime, task, 0), 0);
        assert_eq!(word(&runtime, task, 12), 1);
        assert_eq!(word(&runtime, task, 16), value);
        assert_eq!(word(&runtime, task, 20), 0);
        assert_eq!(word(&runtime, task, 24), 0);
        assert_eq!(word(&runtime, task, 28), 1);
        assert_eq!(word(&runtime, task, 32), 0x1234_5678);
    }
    runtime.destroy(a).unwrap();
    runtime.update(2, 1).unwrap();
    assert_eq!(word(&runtime, b, 36), 0);
    let replacement = runtime.create(0, kind::ORDINARY, b"c", b"").unwrap();
    assert_eq!(replacement, a);
    runtime.update(3, 1).unwrap();
    assert_eq!(word(&runtime, replacement, 0), 0);
    assert!(runtime.cst_handles().is_empty());
}

#[test]
fn contexts_capture_languages_and_keep_shared_handles_until_explicit_release() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[(3, u32::MAX)]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    call(&mut code, 326, &[(3, 0)]);
    instruction(&mut code, 0x26, &[(3, 12)]);
    call(&mut code, 326, &[(0x13, 0), (3, u32::MAX), (3, u32::MAX)]);
    instruction(&mut code, 0x26, &[(3, 16)]);
    call(&mut code, 112, &[(3, 0), (3, 0)]);
    instruction(&mut code, 0x26, &[(3, 4)]);
    call(&mut code, 65, &[]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 8)]);
    call(&mut code, 65, &[]);
    call(&mut code, 324, &[(0x13, 0)]);
    call(&mut code, 324, &[(3, 0)]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0; 20], 64).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().languages.prefixes = vec![
            b"jp".to_vec(),
            b"".to_vec(),
            b"en".to_vec(),
            b"jp\0ignored".to_vec(),
        ];
        let task = runtime.create(0, kind::ORDINARY, b"cst", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let first = 1;
        let context = runtime.cst_context(first).unwrap();
        assert_eq!(
            context.languages().collect::<Vec<_>>(),
            [b"".as_slice(), b"en", b"jp"]
        );
        assert!(context.script().is_none());
        runtime.host_mut().languages.prefixes = vec![b"cn".to_vec()];
        runtime.update(1, 1).unwrap();
        assert_eq!(runtime.cst_context(first).unwrap().languages().len(), 3);
        let second = 4;
        assert_eq!(
            runtime
                .cst_context(second)
                .unwrap()
                .languages()
                .collect::<Vec<_>>(),
            [b"cn"]
        );
        runtime.update(2, 1).unwrap();
        assert!(runtime.cst_context(first).is_none());
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        let handles: Vec<_> = vm
            .read(0, 20)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(handles, [first, 2, second, 0, u32::MAX]);
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
        runtime.destroy(task).unwrap();
        assert!(runtime.cst_context(second).is_some());
    }
}

#[test]
fn cst_tables_merge_values_preserve_nonadjacent_records_and_clear_independently() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    call(&mut code, 982, &[(0x13, 0)]);
    instruction(&mut code, 0x26, &[(3, 128)]);
    for (slot, index, name, value) in [
        (4, 328, 64, 7),
        (8, 328, 80, u32::MAX),
        (12, 329, 64, 1),
        (16, 329, 80, 0x8000_0000),
        (20, 329, 96, 4),
        (24, 329, 64, 3),
    ] {
        call(&mut code, index, &[(0x13, 0), (3, name), (3, value)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 332, &[(0x13, 0), (3, 80)]);
    instruction(&mut code, 0x26, &[(3, 28)]);
    call(&mut code, 982, &[(0x13, 0)]);
    instruction(&mut code, 0x26, &[(3, 132)]);
    call(&mut code, 65, &[]);
    call(&mut code, 330, &[(0x13, 0)]);
    call(&mut code, 332, &[(0x13, 0), (3, 64)]);
    instruction(&mut code, 0x26, &[(3, 32)]);
    call(&mut code, 982, &[(0x13, 0)]);
    instruction(&mut code, 0x26, &[(3, 136)]);
    call(&mut code, 65, &[]);
    call(&mut code, 331, &[(0x13, 0)]);
    call(&mut code, 982, &[(0x13, 0)]);
    instruction(&mut code, 0x26, &[(3, 140)]);
    for (slot, index) in [(36, 329), (40, 328)] {
        call(&mut code, index, &[(0x13, 0), (3, 112)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 329, &[(0x13, 0), (3, 64), (3, 9)]);
    instruction(&mut code, 0x26, &[(3, 44)]);
    call(&mut code, 982, &[(0x13, 0)]);
    instruction(&mut code, 0x26, &[(3, 144)]);
    call(&mut code, 982, &[(3, 0)]);
    instruction(&mut code, 0x26, &[(3, 148)]);
    for (slot, index) in [(48, 332), (52, 328), (56, 329)] {
        call(&mut code, index, &[(3, 0)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    call(&mut code, 330, &[(3, 0)]);
    call(&mut code, 331, &[(3, 0)]);
    code.push(0x3b);
    let mut data = vec![0; 152];
    data[64..70].copy_from_slice(b"A.cst\0");
    data[80..86].copy_from_slice(b"a.alt\0");
    data[96..102].copy_from_slice(b"B.cst\0");
    data[112..120].copy_from_slice(b".hidden\0");
    let program = Program::from_code(&code, data, 64).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"tables", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(
            context.registrations().collect::<Vec<_>>(),
            [(b"A".as_slice(), u32::MAX)]
        );
        assert_eq!(
            context.history().collect::<Vec<_>>(),
            [(b"A".as_slice(), 0x8000_0000), (b"B", 4), (b"A", 3)]
        );
        runtime.update(1, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(context.registrations().len(), 0);
        assert_eq!(context.history().len(), 3);
        runtime.update(2, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(context.registrations().len(), 0);
        assert_eq!(
            context.history().collect::<Vec<_>>(),
            [(b"A".as_slice(), 9)]
        );
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(128, 24).unwrap().as_ref(),
            [u32::MAX, 2, 2, u32::MAX, 0, u32::MAX]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let values: Vec<_> = vm
            .read(4, 56)
            .unwrap()
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(
            values,
            [
                1,
                1,
                0,
                0,
                1,
                2,
                u32::MAX,
                0,
                u32::MAX,
                1,
                0,
                0,
                0,
                u32::MAX
            ]
        );
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
        runtime.destroy(task).unwrap();
        assert_eq!(runtime.cst_context(1).unwrap().history().len(), 1);
    }
}

#[test]
fn registration_aliases_keep_literal_names_existing_values_and_empty_name_fallback() {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[(3, 0x1234_5678)]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    for (index, name, value) in [(328, 128, 7), (328, 192, 3), (329, 128, 99)] {
        call(&mut code, index, &[(0x13, 0), (3, name), (3, value)]);
        instruction(&mut code, 0x2d, &[]);
    }
    for (source, dest) in [
        (160, 224),
        (160, 192),
        (128, 320),
        (160, 256),
        (224, 416),
        (192, 352),
    ] {
        call(&mut code, 934, &[(0x13, 0), (3, source), (3, dest)]);
    }
    call(&mut code, 328, &[(0x13, 0), (3, 288), (3, 0x8000_0000)]);
    instruction(&mut code, 0x2d, &[]);
    call(&mut code, 934, &[(3, 0)]);
    call(&mut code, 934, &[(3, 0), (3, u32::MAX), (3, u32::MAX)]);
    for (slot, name) in [(4, 384), (8, 224), (12, 192)] {
        call(&mut code, 332, &[(0x13, 0), (3, name)]);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    code.push(0x3b);
    let mut data = vec![0; 448];
    for (address, name) in [
        (128, b"alpha.cst".as_slice()),
        (160, b"ALPHA"),
        (192, b"beta"),
        (224, b"copy"),
        (256, b"copy.ext"),
        (288, b"unknown.cst"),
        (320, b"literal"),
        (352, b"after"),
        (384, b"missing"),
    ] {
        data[address..address + name.len()].copy_from_slice(name);
    }
    let program = Program::from_code(&code, data, 64).unwrap();
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"aliases", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let context = runtime.cst_context(1).unwrap();
        assert_eq!(
            context.registrations().collect::<Vec<_>>(),
            [
                (b"alpha".as_slice(), 7),
                (b"beta", 3),
                (b"copy", 7),
                (b"copy.ext", 7),
                (b"", 0x8000_0000),
            ]
        );
        assert_eq!(
            context.history().collect::<Vec<_>>(),
            [(b"alpha".as_slice(), 99)]
        );
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(4, 12).unwrap().as_ref(),
            [0u32, 7, 3].map(u32::to_le_bytes).concat()
        );
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
    }
}

#[test]
fn cst_record_storage_keeps_details_data_and_first_matching_container_record() {
    for budget in [1, 4096] {
        for legacy in [false, true] {
            let mut payload = vec![0; if legacy { 40 } else { 52 }];
            let start = if legacy { 0 } else { 4 };
            payload[start..start + 6].copy_from_slice(b"Alpha\0");
            payload[start + 32..start + 36].copy_from_slice(&7u32.to_le_bytes());
            payload[start + 36..start + 40].copy_from_slice(&1u32.to_le_bytes());
            let mut details: Vec<_> = (0..72u8).collect();
            if !legacy {
                payload[start + 40..start + 44].copy_from_slice(&3u32.to_le_bytes());
                payload[start + 44..start + 48].copy_from_slice(&120u32.to_le_bytes());
            }
            payload.extend(&details);
            if !legacy {
                payload.extend(b"xyz");
            }
            let mut memory = vec![0; 128 + payload.len()];
            memory[64..74].copy_from_slice(b"alpha.cst\0");
            memory[80..82].copy_from_slice(b"x\0");
            memory[128..].copy_from_slice(&payload);
            let mut code = vec![];
            instruction(&mut code, 0x25, &[(3, 0xdeadbeef)]);
            call(&mut code, 323, &[]);
            instruction(&mut code, 0x26, &[(3, 0)]);
            call(&mut code, 238, &[(3, 17)]);
            instruction(&mut code, 0x26, &[(3, 4)]);
            call(
                &mut code,
                244,
                &[(0x13, 4), (3, 7), (3, payload.len() as u32), (3, 128)],
            );
            instruction(&mut code, 0x26, &[(3, 8)]);
            call(&mut code, 386, &[(0x13, 4), (3, 7), (0x13, 0), (3, 1)]);
            instruction(&mut code, 0x26, &[(3, 8)]);
            call(&mut code, 329, &[(0x13, 0), (3, 64), (3, 11)]);
            instruction(&mut code, 0x26, &[(3, 12)]);
            call(
                &mut code,
                334,
                &[
                    (0x13, 0),
                    (3, u32::MAX),
                    (3, 0x03020100),
                    (3, 0x07060504),
                    (3, 80),
                ],
            );
            instruction(&mut code, 0x26, &[(3, 16)]);
            call(
                &mut code,
                335,
                &[(0x13, 0), (3, 0), (3, 0x03020100), (3, 0x07060504), (3, 96)],
            );
            instruction(&mut code, 0x26, &[(3, 16)]);
            for (record, data) in [(8, 1), (9, 0), (8, 0)] {
                call(
                    &mut code,
                    385,
                    &[(0x13, 4), (3, record), (0x13, 0), (3, 1), (3, data)],
                );
                instruction(&mut code, 0x26, &[(3, 16)]);
            }
            call(&mut code, 386, &[(0x13, 4), (3, 999), (0x13, 0), (3, 1)]);
            instruction(&mut code, 0x26, &[(3, 20)]);
            call(
                &mut code,
                385,
                &[(3, 0), (3, 999), (0x13, 0), (3, 1), (3, 1)],
            );
            instruction(&mut code, 0x26, &[(3, 24)]);
            instruction(&mut code, 0x3b, &[]);
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(Program::from_code(&code, memory, 128).unwrap())),
            );
            runtime.set_budget(budget, 1 << 20);
            let caller = runtime.create(0, kind::ORDINARY, b"records", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let vm = runtime.object(caller).unwrap().interpreter().unwrap();
            assert_eq!(
                vm.read(8, 20).unwrap(),
                [1u32, 0, 1, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(vm.stack_bytes(), 0xdeadbeefu32.to_le_bytes());
            assert_eq!(vm.read(96, 2).unwrap(), b"x\0");
            let context = runtime.cst_context(1).unwrap();
            assert_eq!(
                context.history().collect::<Vec<_>>(),
                [(b"Alpha".as_slice(), 11)]
            );
            assert_eq!(context.registrations().len(), 0);
            let container = runtime.host().data_container(2).unwrap();
            let saved = container.record(8).unwrap().unwrap();
            let mut expected = vec![0; 52];
            expected[4..10].copy_from_slice(b"Alpha\0");
            expected[36..40].copy_from_slice(&11u32.to_le_bytes());
            expected[40..44].copy_from_slice(&1u32.to_le_bytes());
            if !legacy {
                expected[44..48].copy_from_slice(&3u32.to_le_bytes());
                expected[48..52].copy_from_slice(&120u32.to_le_bytes());
            }
            details[8..10].copy_from_slice(b"x\0");
            expected.extend(&details);
            let without_data = {
                let mut bytes = expected.clone();
                bytes[44..52].fill(0);
                bytes
            };
            if !legacy {
                expected.extend(b"xyz");
            }
            assert_eq!(saved, expected);
            assert_eq!(container.record(9).unwrap().unwrap(), without_data);
        }
    }
}

#[test]
fn cst_history_strings_use_global_position_fallback_and_preserve_raw_marker_punctuation() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[(3, 0xdeadbeef)]);
    call(&mut code, 323, &[]);
    instruction(&mut code, 0x26, &[(3, 0)]);
    for (slot, index, args) in [
        (8, 333, vec![0, 280, 276]),
        (12, 335, vec![0, 0, 3, 288]),
        (16, 329, vec![128, 1]),
        (20, 334, vec![u32::MAX, 0, 3, 64]),
        (24, 335, vec![0, 1, 4, 300]),
        (28, 334, vec![0, 5, 4, 96]),
        (32, 335, vec![0, 7, 3, 320]),
        (36, 334, vec![0, 8, 3, 512]),
        (40, 335, vec![0, 9, 3, 352]),
    ] {
        let args: Vec<_> = [(0x13, 0)]
            .into_iter()
            .chain(args.into_iter().map(|n| (3, n)))
            .collect();
        call(&mut code, index, &args);
        instruction(&mut code, 0x26, &[(3, slot)]);
    }
    for index in [336, 337] {
        call(
            &mut code,
            index,
            &[
                (0x13, 0),
                (3, u32::MAX),
                (3, 7),
                (3, if index == 336 { 160 } else { 200 }),
            ],
        );
        call(&mut code, index, &[(3, 0)]);
    }
    call(
        &mut code,
        335,
        &[(0x13, 0), (3, 0), (3, 7), (3, 3), (3, 416)],
    );
    instruction(&mut code, 0x26, &[(3, 44)]);
    call(
        &mut code,
        333,
        &[(0x13, 0), (3, u32::MAX), (3, 256), (3, 272)],
    );
    instruction(&mut code, 0x26, &[(3, 48)]);
    instruction(&mut code, 0x3b, &[]);
    let mut data = vec![0; 1024];
    for (address, bytes) in [
        (64, b"zero".as_slice()),
        (96, b"late"),
        (128, b"A.cst"),
        (160, b"$str3;$str8/$str4"),
        (200, b"$str3;$str8/$str4"),
        (288, b"KEEP"),
        (300, b"X"),
        (320, b"X"),
    ] {
        data[address..address + bytes.len()].copy_from_slice(bytes);
    }
    data[512..582].fill(b'x');
    let program = Program::from_code(&code, data, 128).unwrap();
    for budget in [1, 4096] {
        let mut state = State::new(0);
        state.strings.extend([(3, b"GLOBAL".to_vec()), (8, vec![])]);
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            state,
            KcsFactory::new(Loader(program.clone())),
        );
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"details", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(task).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(8, 44).unwrap(),
            [u32::MAX, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(vm.read(276, 4).unwrap(), u32::MAX.to_le_bytes());
        assert_eq!(vm.read(280, 1).unwrap(), [0]);
        assert_eq!(vm.read(288, 5).unwrap(), b"KEEP\0");
        assert_eq!(vm.read(300, 1).unwrap(), [0]);
        assert_eq!(vm.read(320, 1).unwrap(), [0]);
        assert_eq!(
            vm.read(352, 63).unwrap(),
            [vec![b'x'; 62], vec![0]].concat()
        );
        assert_eq!(vm.read(416, 7).unwrap(), b"GLOBAL\0");
        assert_eq!(vm.read(200, 14).unwrap(), b"GLOBAL;/late\0\0");
        assert_eq!(vm.read(256, 2).unwrap(), b"A\0");
        assert_eq!(vm.read(272, 4).unwrap(), 1u32.to_le_bytes());
        assert_eq!(vm.stack_bytes(), 0xdeadbeefu32.to_le_bytes());
    }
}

#[test]
fn legacy_cst_uses_target_history_and_copies_tables_independently() -> Result<()> {
    struct Programs;
    impl ProgramLoader for Programs {
        fn load(&mut self, role: &[u8]) -> Result<Option<Program>> {
            let mut code = vec![];
            let mut data = vec![0; 352];
            for (address, bytes) in [
                (96, b"entry.cst".as_slice()),
                (128, b"direct"),
                (144, b"x$str3/$str8/$str4"),
                (176, b"x$str3/$str8/$str4"),
                (208, b"x$str3/$str8/$str4"),
            ] {
                data[address..address + bytes.len()].copy_from_slice(bytes);
            }
            let mut emit = |index, args: &[u32], result: Option<u32>| {
                let args: Vec<_> = args.iter().map(|&n| (3, n)).collect();
                call(&mut code, index, &args);
                if let Some(slot) = result {
                    instruction(&mut code, 0x26, &[(3, slot)]);
                }
            };
            if role == b"source" {
                emit(219, &[], None);
                emit(223, &[96, 19], Some(0));
                emit(224, &[96, 23], Some(4));
                emit(229, &[0, 7, 4, 128], Some(8));
                emit(231, &[0, 7, 144], None);
                emit(65, &[], None);
                emit(225, &[], None);
                emit(226, &[], None);
            } else {
                emit(234, &[1], Some(0));
                emit(219, &[], None);
                emit(228, &[0, 256, 288], Some(4));
                emit(233, &[1, 0, 7, 176], None);
                emit(234, &[1], Some(8));
                emit(65, &[], None);
                emit(227, &[96], Some(12));
                emit(228, &[u32::MAX, 272, 292], Some(16));
                emit(230, &[0, 7, 4, 304], Some(20));
                emit(232, &[0, 7, 208], None);
                emit(235, &[320], Some(24));
            }
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, data, 96)?))
        }
    }

    for budget in [1, 4096] {
        let mut numbers = State::new(0);
        numbers.strings.insert(3, b"global".to_vec());
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            numbers,
            KcsFactory::new(Programs),
        );
        runtime.set_budget(budget, 1 << 20);
        let source = runtime.create(0, kind::ORDINARY, b"source", b"")?;
        assert_eq!(source, 1);
        let target = runtime.create(0, kind::ORDINARY, b"target", b"")?;
        runtime.update(0, 1)?;
        runtime.update(1, 1)?;
        let vm = runtime.object(target).unwrap().interpreter().unwrap();
        let values: Vec<_> = vm
            .read(0, 28)?
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(values, [0, u32::MAX, 1, 19, 0, 1, 0]);
        assert_eq!(vm.read(176, 22)?, b"xglobal/$str8/direct\0\0".as_slice());
        assert_eq!(vm.read(208, 17)?, b"xglobal//direct\0\0".as_slice());
        assert_eq!(vm.read(256, 1)?, [0]);
        assert_eq!(vm.read(272, 10)?, b"entry.cst\0".as_slice());
        assert_eq!(vm.read(288, 4)?, u32::MAX.to_le_bytes());
        assert_eq!(vm.read(292, 4)?, 23u32.to_le_bytes());
        assert_eq!(vm.read(304, 7)?, b"direct\0".as_slice());
        assert_eq!(vm.read(320, 1)?, [0]);
    }
    Ok(())
}

#[test]
fn legacy_css_restores_only_the_selected_table_in_the_target_task() -> Result<()> {
    struct Programs;
    impl ProgramLoader for Programs {
        fn load(&mut self, role: &[u8]) -> Result<Option<Program>> {
            let mut code = vec![];
            let mut data = vec![0; 320];
            data[64..74].copy_from_slice(b"entry.cst\0");
            data[96..103].copy_from_slice(b"Saved\0\0");
            data[128..132].copy_from_slice(&31u32.to_le_bytes());
            data[132..136].copy_from_slice(&1u32.to_le_bytes());
            data[136..140].copy_from_slice(&3u32.to_le_bytes());
            data[140..144].copy_from_slice(&120u32.to_le_bytes());
            data[144..148].copy_from_slice(&7u32.to_le_bytes());
            data[148..152].copy_from_slice(&3u32.to_le_bytes());
            data[152..158].copy_from_slice(b"short\0");
            data[216..219].copy_from_slice(b"raw");
            let mut emit = |index, args: &[(u8, u32)], result: Option<u32>| {
                call(&mut code, index, args);
                if let Some(address) = result {
                    instruction(&mut code, 0x26, &[(3, address)]);
                }
            };
            emit(219, &[], None);
            if role == b"target" {
                emit(223, &[(3, 64), (3, 19)], Some(0));
                emit(224, &[(3, 64), (3, 23)], Some(4));
            } else {
                emit(238, &[(3, 17)], Some(0));
                emit(239, &[(3, 0)], Some(4));
                emit(249, &[(3, 0)], Some(8));
                emit(
                    239,
                    &[(3, u32::MAX), (3, 7), (3, 99), (3, 1), (3, 1)],
                    Some(12),
                );
                emit(249, &[(0x13, 0), (3, 99), (3, 1), (3, 1)], Some(16));
                for (address, length) in [(92, 127), (220, 4)] {
                    emit(
                        244,
                        &[(0x13, 0), (3, 7), (3, length), (3, address)],
                        Some(20),
                    );
                }
                emit(249, &[(0x13, 0), (3, 7), (3, 1), (3, 7)], Some(24));
                for (record, selector, data, output) in
                    [(8, 0, 1, 28), (9, 1, 0, 32), (10, 1, 1, 36)]
                {
                    emit(
                        239,
                        &[(0x13, 0), (3, record), (3, 1), (3, selector), (3, data)],
                        Some(output),
                    );
                }
                emit(227, &[(3, 64)], Some(40));
            }
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, data, 64)?))
        }
    }

    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Programs),
    );
    let target = runtime.create(0, kind::ORDINARY, b"target", b"")?;
    assert_eq!(target, 1);
    let caller = runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    runtime.update(0, 1)?;
    let vm = runtime.object(caller).unwrap().interpreter().unwrap();
    let values: Vec<_> = vm
        .read(0, 44)?
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(&values[1..], &[0, 0, 0, 0, 1, 1, 1, 1, 1, 0]);
    let css = runtime.host().data_container(values[0])?;
    let registration = css.record(8)?.unwrap();
    assert_eq!(&registration[4..14], b"entry.cst\0");
    assert_eq!(&registration[36..40], &19u32.to_le_bytes());
    let first = css.record(7)?.unwrap();
    assert_eq!(css.record(10)?, Some(first));
    let mut without_data = first[..124].to_vec();
    without_data[44..52].fill(0);
    assert_eq!(css.record(9)?, Some(without_data.as_slice()));
    Ok(())
}
