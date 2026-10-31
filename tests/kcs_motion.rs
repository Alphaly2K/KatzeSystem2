use katze_system2::{
    Step,
    kcs::{Host, Interpreter, MotionHost, MotionState, MotionTask, NativeReturn, Program},
};

fn vm() -> Interpreter {
    Interpreter::new(
        Program::from_code(&[], vec![0; 512], 64).unwrap(),
        MotionState::default().external_data(),
    )
    .unwrap()
}

fn call(host: &mut MotionHost, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    let bytes: Vec<_> = args.iter().flat_map(|n| n.to_le_bytes()).collect();
    host.call(index, &bytes, vm).unwrap()
}

fn words(vm: &Interpreter, address: u32, count: usize) -> Vec<u32> {
    vm.read(address, count * 4)
        .unwrap()
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn inverse_and_normalization_preserve_sse_rounding_and_singular_matrices() {
    let mut vm = vm();
    let mut host = MotionHost::default();
    call(&mut host, &mut vm, 26, &[0]);
    call(&mut host, &mut vm, 27, &[0]);
    let expected: Vec<_> = (0..16)
        .map(|i| if i % 5 == 0 { 0x3f7f_ffff } else { 0 })
        .collect();
    assert_eq!(words(&vm, 0, 16), expected);
    call(&mut host, &mut vm, 27, &[64]);
    assert_eq!(words(&vm, 64, 16), [0; 16]);
    vm.write(128, &1.0f32.to_le_bytes()).unwrap();
    call(&mut host, &mut vm, 31, &[128]);
    assert_eq!(words(&vm, 128, 4), [0x3f7f_ffff, 0, 0, 0]);
    call(&mut host, &mut vm, 31, &[144]);
    assert_eq!(words(&vm, 144, 4), [0xffc0_0000; 4]);
}

#[test]
fn matrix_copy_keeps_the_sequential_block_overlap_behavior() {
    let mut vm = vm();
    let mut host = MotionHost::default();
    let input: Vec<_> = (0..20u32).flat_map(u32::to_le_bytes).collect();
    vm.write(0, &input).unwrap();
    call(&mut host, &mut vm, 28, &[4, 0]);
    assert_eq!(
        words(&vm, 4, 16),
        [0, 1, 2, 3, 3, 5, 6, 7, 7, 9, 10, 11, 11, 13, 14, 15]
    );
}

#[test]
fn yaw_pitch_roll_uses_immediate_angles_and_the_sse_quaternion_path() {
    let mut vm = vm();
    let mut host = MotionHost::default();
    let initial: Vec<_> = (0..128)
        .flat_map(|i| ((((i * 3 + 8 * 17) % 31) as f64 / 7.0 - 2.0) as f32).to_le_bytes())
        .collect();
    vm.write(0, &initial).unwrap();
    call(&mut host, &mut vm, 43, &[128, 400, 400, 1.25f32.to_bits()]);
    assert_eq!(
        words(&vm, 128, 16),
        [
            0xbefe_950d,
            0x3ea1_aa70,
            0x3f80_0000,
            0x3fb6_db6e,
            0xbfca_b06e,
            0x401e_ebb7,
            0xbfdb_6db7,
            0xbfa4_9249,
            0x3e0b_b4aa,
            0xbf72_d468,
            0x8000_01b2,
            0x3edb_6db7,
            0xbf73_2928,
            0x3f9c_029e,
            0x3fdb_6db7,
            0x4009_2492,
        ]
    );
}

#[test]
fn motion_exit_updates_external_state_and_validates_parameter_storage() {
    let mut vm = vm();
    let mut host = MotionHost::default();
    assert_eq!(call(&mut host, &mut vm, 38, &[]), NativeReturn::YieldVoid);
    assert_eq!(MotionState::read(&vm).unwrap().running, 0);
    assert!(host.call(38, &[0; 4], &mut vm).is_err());
    assert!(host.call(45, &[], &mut vm).is_err());
    let state = MotionState::with_parameters(&[f32::from_bits(0x7fc0_1234), -0.0]).unwrap();
    let vm = Interpreter::new(
        Program::from_code(&[], vec![], 0).unwrap(),
        state.external_data(),
    )
    .unwrap();
    assert_eq!(MotionState::read(&vm).unwrap(), state);
    assert!(MotionState::with_parameters(&[0.0; 257]).is_err());
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn lifecycle_program(exit: bool) -> Program {
    let mut code = vec![];
    ins(&mut code, 0x16, &[(3, 0)]);
    code.push(0x2d);
    for (target, source) in [
        (0, 0),
        (4, 0x8000_0090),
        (8, 0x8000_0094),
        (12, 0x8000_0098),
    ] {
        ins(
            &mut code,
            0x15,
            &[(3, 0x8000_0000 + target), (0x13, source)],
        );
        code.push(0x2d);
    }
    if exit {
        ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    }
    code.push(0x3b);
    Program::from_code(&code, vec![0; 32], 64).unwrap()
}

#[test]
fn motion_updates_restart_the_entry_and_preserve_data_after_halt() {
    let mut task = MotionTask::new(lifecycle_program(false), MotionState::default(), 10).unwrap();
    let mut host = MotionHost::default();
    assert_eq!(task.update(9, &mut host, 100).unwrap(), None);
    assert_eq!(task.interpreter.pc, 0);
    for (now, count) in [(10, 1), (15, 2), (20, 3)] {
        assert_eq!(
            task.update(now, &mut host, 100).unwrap(),
            Some(Step::Halted)
        );
        let state = task.state().unwrap();
        assert_eq!(state.properties[..4], [count, now - 10, 1, 0]);
        assert_eq!(state.running, 1);
        assert!(task.interpreter.stack_bytes().is_empty());
    }
    task.interpreter
        .write(0x8000_0098, &1u32.to_le_bytes())
        .unwrap();
    task.update(0, &mut host, 100).unwrap();
    assert_eq!(task.state().unwrap().elapsed, u32::MAX - 9);
}

#[test]
fn motion_budget_resume_keeps_time_and_finish_restores_only_control_fields() {
    let state = MotionState {
        elapsed: 99,
        running: -2,
        ..Default::default()
    };
    let mut task = MotionTask::new(lifecycle_program(true), state, 10).unwrap();
    let mut host = MotionHost::default();
    assert_eq!(task.update(12, &mut host, 1).unwrap(), Some(Step::Yield));
    assert_eq!(
        task.update(100, &mut host, 100).unwrap(),
        Some(Step::Event(katze_system2::kcs::Event {
            native_index: 38,
            status: 4
        }))
    );
    assert_eq!(
        task.state().unwrap().properties[..4],
        [1, 2, (-2i32) as u32, 0]
    );
    let saved = task.state().unwrap();
    assert_eq!(task.finish(&mut host, 1).unwrap(), Step::Yield);
    assert!(task.update(200, &mut host, 100).is_err());
    assert!(matches!(
        task.finish(&mut host, 100).unwrap(),
        Step::Event(_)
    ));
    let state = task.state().unwrap();
    assert_eq!(state.properties[..4], [2, u32::MAX, 0, 1]);
    assert_eq!(
        (state.elapsed, state.running, state.finishing),
        (saved.elapsed, saved.running, saved.finishing)
    );
    assert_eq!(words(&task.interpreter, 0, 1), [2]);

    struct FailingHost;
    impl Host for FailingHost {
        fn call(
            &mut self,
            _: u32,
            _: &[u8],
            _: &mut Interpreter,
        ) -> katze_system2::Result<NativeReturn> {
            Err(katze_system2::Error::new(0, "synthetic failure"))
        }
    }
    assert!(task.finish(&mut FailingHost, 100).is_err());
    let state = task.state().unwrap();
    assert_eq!(
        (state.elapsed, state.running, state.finishing),
        (saved.elapsed, saved.running, saved.finishing)
    );
}

#[test]
fn reset_discards_a_suspended_call_frame_without_reinitializing_data() {
    let mut code = vec![];
    ins(&mut code, 0x2a, &[(3, 12), (3, 0)]);
    code.push(0x3b);
    ins(&mut code, 0x16, &[(3, 0)]);
    code.push(0x2d);
    ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    ins(&mut code, 0x2c, &[(3, 0), (3, 0)]);
    let mut task = MotionTask::new(
        Program::from_code(&code, vec![0; 4], 64).unwrap(),
        MotionState::default(),
        0,
    )
    .unwrap();
    let mut host = MotionHost::default();
    for now in 0..2 {
        assert!(matches!(
            task.update(now, &mut host, 100).unwrap(),
            Some(Step::Event(_))
        ));
        assert_eq!(words(&task.interpreter, 0, 1), [now + 1]);
        assert_eq!(task.interpreter.stack_bytes().len(), 12);
    }
    assert_eq!(task.interpreter.run(&mut host, 100).unwrap(), Step::Halted);
    assert!(task.interpreter.stack_bytes().is_empty());
}

#[test]
fn reference_axis_rotation_reads_three_addresses_and_preserves_matrix_order() {
    let mut vm = vm();
    let mut host = MotionHost::default();
    call(&mut host, &mut vm, 26, &[128]);
    call(&mut host, &mut vm, 26, &[256]);
    for (offset, angle) in [(400, 1.0f32), (404, 10.0 / 7.0), (408, 13.0 / 7.0)] {
        vm.write(offset, &angle.to_le_bytes()).unwrap();
    }
    call(&mut host, &mut vm, 44, &[128, 256, 400]);
    assert_eq!(
        words(&vm, 128, 16),
        [
            0xbd23_fcd1,
            0xbe0b_3cd6,
            0x3f7d_6a45,
            0,
            0x3e90_e931,
            0xbf73_a05d,
            0xbdf4_4673,
            0,
            0x3f75_5183,
            0x3e8d_00ba,
            0x3d9c_d8e9,
            0,
            0,
            0,
            0,
            0x3f80_0000,
        ]
    );
}
