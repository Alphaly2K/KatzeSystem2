use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    settings::SystemSettings,
};

fn arguments(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn flag_range_outputs_keep_selector_bits_overlap_and_partial_failure_order() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xaa; 64], 0).unwrap(),
        vec![0xbb; 16],
    )
    .unwrap();
    assert_eq!(
        host.call(790, &arguments(&[0, u32::MAX, u32::MAX, u32::MAX]), &mut vm)
            .unwrap(),
        NativeReturn::Value(0)
    );
    assert!(host.call(790, &arguments(&[0, 0, 0, 4]), &mut vm).is_err());
    let mut settings = SystemSettings::default();
    settings.local_flag_ranges[7] = [0x11223344, 0x55667788];
    settings.global_flag_ranges[7] = [0x80000000, u32::MAX];
    host.system_settings = Some(settings);
    assert_eq!(
        host.call(790, &arguments(&[0, 7, 0, 1]), &mut vm).unwrap(),
        NativeReturn::Value(1)
    );
    assert_eq!(vm.read(0, 6).unwrap(), [0x44, 0x88, 0x77, 0x66, 0x55, 0xaa]);
    host.call(790, &arguments(&[u32::MAX, 7, 0x80000000, 8]), &mut vm)
        .unwrap();
    assert_eq!(vm.read(0x80000000, 4).unwrap(), 0x80000000u32.to_le_bytes());
    assert_eq!(vm.read(8, 4).unwrap(), u32::MAX.to_le_bytes());
    host.call(790, &arguments(&[0, 7, 16, 16]), &mut vm)
        .unwrap();
    assert_eq!(vm.read(16, 4).unwrap(), 0x55667788u32.to_le_bytes());
    assert!(
        host.call(790, &arguments(&[0, 7, 20, 0x8000000e]), &mut vm)
            .is_err()
    );
    assert_eq!(vm.read(20, 4).unwrap(), 0x11223344u32.to_le_bytes());
    assert!(host.call(790, &arguments(&[0, 7, 24]), &mut vm).is_err());
    assert_eq!(vm.read(24, 4).unwrap(), [0xaa; 4]);
    let snapshot = vm.read(0, 64).unwrap().to_vec();
    assert_eq!(
        host.call(790, &arguments(&[1, 8, 0, 4]), &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    assert_eq!(vm.read(0, 64).unwrap(), snapshot);
}

#[test]
fn native_output_can_rewrite_the_popped_index_before_reading_the_count() {
    fn ins(code: &mut Vec<u8>, op: u8, values: &[u32]) {
        code.push(op);
        for value in values {
            code.push(3);
            code.extend(value.to_le_bytes());
        }
    }
    let mut code = vec![];
    for value in [0, 0, 132, 0x80000000] {
        ins(&mut code, 0x25, &[value]);
    }
    ins(&mut code, 0x2b, &[790, 16]);
    ins(&mut code, 0x3b, &[]);
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut settings = SystemSettings::default();
    settings.local_flag_ranges[0] = [1, 11];
    settings.local_flag_ranges[1] = [3, 22];
    host.system_settings = Some(settings);
    let mut vm = Interpreter::new(
        Program::from_code(&code, vec![0; 128], 64).unwrap(),
        vec![0; 4],
    )
    .unwrap();
    vm.run(&mut host, 16).unwrap();
    assert_eq!(vm.read(132, 4).unwrap(), 1u32.to_le_bytes());
    assert_eq!(vm.read(0x80000000, 4).unwrap(), 22u32.to_le_bytes());
    assert_eq!(vm.stack_bytes(), 1u32.to_le_bytes());

    // 离开原调用后，普通宿主调用继续使用其显式参数。
    host.call(790, &arguments(&[0, 0, 8, 0x80000000]), &mut vm)
        .unwrap();
    assert_eq!(vm.read(0x80000000, 4).unwrap(), 11u32.to_le_bytes());

    host.system_settings.as_mut().unwrap().local_flag_ranges[0][0] = 8;
    vm.reset_execution();
    assert!(vm.run(&mut host, 16).is_err());
    host.call(790, &arguments(&[0, 0, 8, 0x80000000]), &mut vm)
        .unwrap();
    assert_eq!(vm.read(0x80000000, 4).unwrap(), 11u32.to_le_bytes());
}

#[test]
fn flag_backup_copies_the_slot_count_in_bytes_and_keeps_other_banks() {
    let mut numbers = State::new(9);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![], 0).unwrap(), vec![]).unwrap();
    host.state.shared[..3].copy_from_slice(&[0x11223344, 0x55667788, 0x99aabbccu32 as i32]);
    host.state.local[0] = 12;
    host.state.current = 34;
    host.state.strings.insert(0, b"saved".to_vec());
    assert!(host.call(610, &[], &mut vm).is_err());
    assert_eq!(
        host.call(609, &[], &mut vm).unwrap(),
        NativeReturn::Continue
    );
    host.state.shared.fill(0xdeadbeefu32 as i32);
    assert!(host.call(609, &[0; 4], &mut vm).is_err());
    for _ in 0..2 {
        assert_eq!(
            host.call(610, &[], &mut vm).unwrap(),
            NativeReturn::Continue
        );
        assert_eq!(
            host.state.shared[..3],
            [0x11223344, 0x55667788, 0xdeadbeccu32 as i32]
        );
        assert_eq!(host.state.shared[3..], [0xdeadbeefu32 as i32; 6]);
        host.state.shared[0] = 0;
    }
    host.state.shared[0] = 99;
    host.call(609, &[], &mut vm).unwrap();
    host.state.shared[0] = 100;
    host.call(610, &[], &mut vm).unwrap();
    assert_eq!(host.state.shared[0], 99);
    assert_eq!(host.state.local[0], 12);
    assert_eq!(host.state.current, 34);
    assert_eq!(host.state.strings[&0], b"saved");
    assert_eq!(host.configured_shared_slots, 9);
    host.state.shared.push(0);
    assert!(host.call(610, &[], &mut vm).is_err());
    let mut empty = State::new(0);
    assert_eq!(
        CoreHost::new(&mut empty).call(610, &[], &mut vm).unwrap(),
        NativeReturn::Continue
    );
}

#[test]
fn runtime_flag_backup_is_shared_between_tasks_and_skips_unbound_callers() {
    use katze_system2::{
        Result,
        runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
        task::{Command, CommandBuffers},
    };
    struct Loader;
    impl ProgramLoader for Loader {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            let natives: &[u32] = match name {
                b"backup" => &[609],
                b"restore" => &[610],
                _ => &[610, 609],
            };
            let mut code = Vec::new();
            for native in natives {
                code.extend([0x2b, 3]);
                code.extend(native.to_le_bytes());
                code.push(3);
                code.extend(0u32.to_le_bytes());
            }
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, vec![], 16)?))
        }
    }
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(5),
            KcsFactory::new(Loader),
        );
        runtime.set_budget(budget, 1 << 20);
        runtime
            .numbers_mut()
            .shared
            .copy_from_slice(&[0x11223344, 0x55667788, 3, 4, 5]);
        let creator = runtime.create(0, kind::ORDINARY, b"backup", b"").unwrap();
        runtime.update(0, 0).unwrap();
        runtime.destroy(creator).unwrap();
        runtime.numbers_mut().shared.fill(0xaabbccddu32 as i32);
        let unbound = runtime.create_unloaded_ordinary(0, b"").unwrap();
        runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    target: unbound,
                    sender: 0,
                    code: 161,
                    data: [0, 1, 0],
                },
                &mut CommandBuffers {
                    data1: Some(&mut b"unbound\0".to_vec()),
                    ..Default::default()
                },
            )
            .unwrap();
        runtime.update(1, 0).unwrap();
        assert_eq!(runtime.numbers().shared, [0xaabbccddu32 as i32; 5]);
        runtime.create(0, kind::ORDINARY, b"restore", b"").unwrap();
        runtime.update(2, 0).unwrap();
        assert_eq!(
            runtime.numbers().shared,
            [
                0x11223344,
                0xaabbcc88u32 as i32,
                0xaabbccddu32 as i32,
                0xaabbccddu32 as i32,
                0xaabbccddu32 as i32
            ]
        );
    }
}
