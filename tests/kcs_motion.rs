use katze_system2::kcs::{Host, Interpreter, MotionHost, MotionState, NativeReturn, Program};

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
