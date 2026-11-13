use katze_system2::{Result, Step, kcs};

struct NoHost;
impl kcs::Host for NoHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut kcs::Interpreter) -> Result<kcs::NativeReturn> {
        panic!("unexpected native call")
    }
}

#[test]
fn rectangle_intersection_uses_signed_endpoints_and_clears_empty_results() -> Result<()> {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(kcs::Program::from_code(&[], vec![0; 16], 64)?, vec![])?;
    let mut numbers = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut numbers);
    for (source, clip, expected) in [
        ([-5, -2, 10, 20], [0, 0, 50, 15], [0, 0, 10, 15]),
        ([0, 0, 10, 10], [10, 0, 20, 10], [0; 4]),
        ([8, 3, 2, 10], [0, 0, 20, 20], [0; 4]),
        ([0, 0, 20, 20], [8, 3, 2, 10], [0; 4]),
        ([2, 3, 4, 5], [-1, -2, 10, 10], [2, 3, 4, 5]),
        ([2, 3, 4, 3], [-1, -2, 10, 10], [0; 4]),
        (
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
            [-10, -20, 30, 40],
            [-10, -20, 30, 40],
        ),
    ] {
        let bytes: Vec<_> = source.into_iter().flat_map(i32::to_le_bytes).collect();
        vm.write(0, &bytes)?;
        let arguments: Vec<_> = [0]
            .into_iter()
            .chain(clip.map(|n| n as u32))
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(
            host.call(218, &arguments, &mut vm)?,
            kcs::NativeReturn::Continue
        );
        let expected: Vec<_> = expected.into_iter().flat_map(i32::to_le_bytes).collect();
        assert_eq!(vm.read(0, 16)?, expected);
    }
    let before = vm.read(0, 16)?.to_vec();
    assert!(host.call(218, &[0; 16], &mut vm).is_err());
    let mut invalid = [0; 20];
    invalid[..4].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
    assert!(host.call(218, &invalid, &mut vm).is_err());
    assert_eq!(vm.read(0, 16)?, before);
    Ok(())
}

#[test]
fn spline_preparation_keeps_control_points_boundary_coefficients_and_guard_bytes() -> Result<()> {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(kcs::Program::from_code(&[], vec![0xa5; 88], 64)?, vec![])?;
    let mut numbers = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut numbers);
    let input: Vec<_> = (0..4)
        .flat_map(|i| [i as f32, (i * i * i) as f32, 0., 0., 0.].map(f32::to_bits))
        .flat_map(u32::to_le_bytes)
        .collect();
    vm.write(4, &input)?;
    let arguments = |address: u32, count: u32| {
        [address, count]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    };
    for count in [0, 1, 2, u32::MAX] {
        assert_eq!(
            host.call(320, &arguments(4, count), &mut vm)?,
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read(4, 80)?, input);
    }
    assert_eq!(
        host.call(320, &arguments(4, 4), &mut vm)?,
        kcs::NativeReturn::Continue
    );
    let expected = [
        0,
        0,
        0x3f2f_fffc,
        0xbf97_fffe,
        0x3fc0_0000,
        0x3f80_0000,
        0x3f80_0000,
        0x4034_0000,
        0x4054_0001,
        0x3f5f_fffe,
        0x4000_0000,
        0x4100_0000,
        0x4141_0000,
        0x40be_0000,
        0x3f80_0003,
        0x4040_0000,
        0x41d8_0000,
        0x41d7_8000,
        0x403e_aaac,
        0x3f80_0003,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    assert_eq!(vm.read(4, 80)?, expected);
    assert_eq!(vm.read(0, 4)?, [0xa5; 4]);
    assert_eq!(vm.read(84, 4)?, [0xa5; 4]);
    let before = vm.read(0, 88)?.to_vec();
    assert!(host.call(320, &arguments(0x7fff_ffff, 5), &mut vm).is_err());
    assert!(host.call(320, &[0; 4], &mut vm).is_err());
    assert_eq!(vm.read(0, 88)?, before);
    Ok(())
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[(u8, u32)]) {
    code.push(op);
    for &(flags, n) in args {
        code.push(flags);
        code.extend(n.to_le_bytes());
    }
}

fn execute(op: u8, args: &[(u8, u32)], data: u32) -> Result<(u32, u32)> {
    let mut code = Vec::new();
    ins(&mut code, op, args);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&code, data.to_le_bytes().to_vec(), 64)?,
        vec![],
    )?;
    assert_eq!(vm.run(&mut NoHost, 10)?, Step::Halted);
    Ok((
        u32::from_le_bytes(vm.stack_bytes().try_into().unwrap()),
        u32::from_le_bytes(vm.read(0, 4)?.try_into().unwrap()),
    ))
}

#[test]
fn integer_signedness_shift_and_division_rules() {
    for (op, lf, a, rf, b, expected) in [
        (4, 3, u32::MAX, 3, 2, 0x7fff_ffff),
        (4, 3, u32::MAX, 0x23, 2, 0),
        (5, 0x23, (-7i32) as u32, 3, 3, u32::MAX),
        (6, 3, 0xa5, 3, 0x0f, 5),
        (7, 3, 0xa0, 3, 5, 0xa5),
        (9, 3, 0xa5, 3, 0x0f, 0xaa),
        (10, 3, 1, 3, 33, 2),
        (11, 0x23, 0x8000_0000, 0x23, 1, 0x4000_0000),
        (0x0f, 3, u32::MAX, 3, 1, 1),
        (0x0f, 3, u32::MAX, 0x23, 1, 0),
        (0x0d, 4, 0x8000_0000, 3, 1, 1),
        (4, 3, 7, 3, 0, 0),
        (5, 3, 7, 3, 0, 0),
    ] {
        assert_eq!(execute(op, &[(lf, a), (rf, b)], 0).unwrap().0, expected);
    }
    for op in [4, 5] {
        assert!(execute(op, &[(0x23, 0x8000_0000), (3, u32::MAX)], 0).is_err());
    }
}

#[test]
fn float_bit_patterns_and_conversions_match_x86() {
    for (op, left, right, expected) in [
        (1, 1.5f32.to_bits(), 2f32.to_bits(), 3.5f32.to_bits()),
        (1, 0x7f80_1234, 0x7fc0_5678, 0x7fc0_1234),
        (3, 0x7f80_0000, 0, 0xffc0_0000),
        (4, 1f32.to_bits(), 0x8000_0000, 0),
        (0x13, 0x7fc0_1234, 0x7fc0_1234, 0),
        (0x14, 0x7fc0_1234, 0x7fc0_1234, 1),
        (0x10, 0x7fc0_1234, 0, 0),
    ] {
        assert_eq!(
            execute(op, &[(4, left), (4, right)], 0).unwrap().0,
            expected
        );
    }
    // 算术读取两侧的浮点位，比较只读取左侧，均不自动转换数值。
    assert_eq!(
        execute(1, &[(3, 0x3f80_0000), (4, 0x3f80_0000)], 0)
            .unwrap()
            .0,
        0x4000_0000
    );
    assert_eq!(
        execute(0x13, &[(3, 0x7fc0_1234), (4, 0x7fc0_1234)], 0)
            .unwrap()
            .0,
        1
    );
    for (op, input, expected) in [
        (0x36, u32::MAX, 4294967296f32.to_bits()),
        (0x37, u32::MAX, (-1f32).to_bits()),
        (0x38, (-1.5f32).to_bits(), u32::MAX),
        (0x38, 4294967296f32.to_bits(), 0),
        (0x38, (-4294967040f32).to_bits(), 256),
        (0x38, f32::NAN.to_bits(), 0),
        (0x39, 2147483648f32.to_bits(), 0x8000_0000),
        (0x39, f32::INFINITY.to_bits(), 0x8000_0000),
    ] {
        assert_eq!(execute(op, &[(4, input)], 0).unwrap().0, expected);
    }
}

#[test]
fn narrow_writes_increment_results_and_compound_division() {
    for (op, flags, initial, rhs_flags, rhs, result, memory) in [
        (0x15, 0x11, 0x1122_33ff, 3, 256, 0, 0x1122_3300),
        (0x1e, 0x11, 0x1122_33ff, 3, 1, 0x1122_3400, 0x1122_3300),
        (0x20, 0x13, 13, 3, 3, 1, 1),
        (0x20, 0x33, 13, 3, 3, 4, 4),
        (
            0x20,
            0x14,
            1f32.to_bits(),
            4,
            0x8000_0000,
            0x8000_0000,
            0x8000_0000,
        ),
        (
            0x1e,
            0x14,
            0x7fc0_1234,
            4,
            0x7fc0_5678,
            0x7fc0_5678,
            0x7fc0_5678,
        ),
        (0x22, 0x33, 0x8000_0000, 3, 1, 0xc000_0000, 0xc000_0000),
        (0x1b, 0x13, 13, 3, 0, 0, 0),
    ] {
        assert_eq!(
            execute(op, &[(flags, 0), (rhs_flags, rhs)], initial).unwrap(),
            (result, memory)
        );
    }
    for (op, result, memory) in [
        (0x16, 0x1122_3300, 0x1122_3300),
        (0x17, 0x1122_33fe, 0x1122_33fe),
        (0x18, 0x1122_33ff, 0x1122_3300),
        (0x19, 0x1122_33ff, 0x1122_33fe),
    ] {
        assert_eq!(
            execute(op, &[(0x91, 0)], 0x1122_33ff).unwrap(),
            (result, memory)
        );
    }
}

#[test]
fn stack_address_writes_and_block_copy_preserve_values() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 0xaabb_ccdd)]);
    ins(&mut code, 0x25, &[(3, 0)]);
    ins(&mut code, 0x26, &[(0xd1, 0)]);
    ins(&mut code, 0x25, &[(3, 0x1234_5678)]);
    ins(&mut code, 0x25, &[(0x43, 999)]);
    ins(&mut code, 0x30, &[(0x83, 4), (3, 4)]);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&code, vec![0; 8], 64).unwrap(),
        vec![],
    )
    .unwrap();
    assert_eq!(vm.run(&mut NoHost, 20).unwrap(), Step::Halted);
    assert_eq!(
        vm.read(0, 8).unwrap(),
        [0xdd, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
}

#[test]
fn reverse_unsigned_switch_range_and_debug_metadata() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 3)]);
    ins(&mut code, 0x33, &[]);
    ins(&mut code, 0x35, &[(3, 10), (3, 2), (3, 29)]);
    ins(&mut code, 0x25, &[(3, 999)]);
    ins(&mut code, 0x41, &[(3, 12), (0x83, 0)]);
    ins(&mut code, 0x42, &[(0x83, 0)]);
    ins(&mut code, 0x43, &[(3, 3), (3, 2)]);
    ins(&mut code, 0x44, &[(3, 1)]);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&code, b"unit\0".to_vec(), 64).unwrap(),
        vec![],
    )
    .unwrap();
    assert_eq!(vm.run(&mut NoHost, 30).unwrap(), Step::Halted);
    assert!(vm.stack_bytes().is_empty());
    assert_eq!(
        vm.debug(),
        &kcs::DebugState {
            line: 12,
            source: b"unit".to_vec(),
            scopes: vec![0],
            variables: vec![3],
            step_over_return: None,
        }
    );
}

#[test]
fn core_host_shares_numeric_state_and_checks_native_contracts() {
    let mut code = Vec::new();
    for (native, args) in [
        (209, vec![2]),
        (211, vec![3, 17]),
        (212, vec![5]),
        (213, vec![0]),
        (213, vec![7]),
        (213, vec![8]),
        (210, vec![3]),
        (210, vec![99]),
    ] {
        for &arg in &args {
            ins(&mut code, 0x25, &[(3, arg)]);
        }
        ins(&mut code, 0x2b, &[(3, native), (3, args.len() as u32 * 4)]);
    }
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&code, b"#3+=@%\0\0%\0".to_vec(), 128).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    assert_eq!(vm.run(&mut host, 100).unwrap(), Step::Halted);
    assert_eq!(
        vm.stack_bytes(),
        [22u32, 0, 0, 22, 0].map(u32::to_le_bytes).concat()
    );
    assert_eq!(host.state.shared.len(), 8);
    assert_eq!(host.state.shared[3], 22);
    assert_eq!(host.state.current, 5);
    assert!(kcs::Host::call(&mut host, 211, &[0; 4], &mut vm).is_err());
    assert!(kcs::Host::call(&mut host, 1000, &[], &mut vm).is_err());
}

#[test]
fn ordinary_identity_keeps_zero_addresses_and_shared_fallback_distinct() {
    let identity = [
        1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
    .map(f32::to_bits);
    let bytes: Vec<_> = identity
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let mut state = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut state);
    assert_eq!(host.implicit_matrix, [0; 16]);
    for address in [0u32, 0x8000_0001, u32::MAX] {
        let mut vm = kcs::Interpreter::new(
            kcs::Program::from_code(&[], vec![0xa5; 80], 0).unwrap(),
            vec![0xb6; 80],
        )
        .unwrap();
        assert_eq!(
            kcs::Host::call(&mut host, 207, &address.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        let mut memory = vec![0xa5; 80];
        let mut external = vec![0xb6; 80];
        match address {
            0 => memory[..64].copy_from_slice(&bytes),
            0x8000_0001 => external[1..65].copy_from_slice(&bytes),
            _ => assert_eq!(host.implicit_matrix, identity),
        }
        if address != u32::MAX {
            assert_eq!(host.implicit_matrix, [0; 16]);
        }
        assert_eq!(vm.read(0, 80).unwrap(), memory);
        assert_eq!(vm.read(0x8000_0000, 80).unwrap(), external);
    }
}

#[test]
fn return_to_code_end_finishes_without_a_stop_marker() {
    let mut code = Vec::new();
    ins(&mut code, 0x2c, &[(3, 0), (3, 0)]);
    ins(&mut code, 0x2a, &[(3, 0), (3, 0)]);
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&code, vec![], 64).unwrap(), vec![]).unwrap();
    vm.pc = 11;
    assert_eq!(vm.run(&mut NoHost, 10).unwrap(), Step::Halted);
    assert_eq!(vm.pc, code.len());
    assert!(vm.stack_bytes().is_empty());
}

#[test]
fn nested_returns_keep_cross_frame_pops_and_temporary_data_area_stack_writes() {
    let mut code = vec![0; 256];
    let mut put = |at: usize, op, operands: &[(u8, u32)]| {
        let mut instruction = Vec::new();
        ins(&mut instruction, op, operands);
        code[at..at + instruction.len()].copy_from_slice(&instruction);
    };
    put(0, 0x25, &[(3, 0x55)]);
    put(6, 0x2a, &[(3, 64), (3, 4)]);
    put(17, 0x3b, &[]);
    put(64, 0x31, &[(3, 4)]);
    put(70, 0x3d, &[(3, 16)]);
    put(76, 0x15, &[(0x73, 0), (0x33, 0x8000_0004)]);
    put(87, 0x3c, &[]);
    put(88, 0x2a, &[(3, 128), (3, 0)]);
    put(99, 0x3c, &[]);
    put(100, 0x3d, &[(3, 16)]);
    put(106, 0x25, &[(0x73, 0)]);
    put(112, 0x2c, &[(3, 4), (3, 4)]);
    put(128, 0x14, &[(0x33, 0x8000_0000), (0xe1, 0)]);
    put(139, 0x27, &[(3, 163)]);
    put(145, 0x15, &[(3, 32), (3, 0x1122_3344)]);
    put(156, 0x3c, &[]);
    put(157, 0x29, &[(3, 163)]);
    put(163, 0x2c, &[(3, 0), (3, 0)]);
    for start in [64, 65] {
        for equal in [false, true] {
            for budget in [1, 4096] {
                let value = 0x1234_5678u32;
                let global = if equal { start as u32 + 20 } else { 0 };
                let mut vm = kcs::Interpreter::new(
                    kcs::Program::from_code(&code, vec![0xa5; start], 512 - start).unwrap(),
                    [global.to_le_bytes(), value.to_le_bytes()].concat(),
                )
                .unwrap();
                let mut halted = false;
                let mut observed_data_stack = false;
                for _ in 0..64 {
                    let step = vm.run(&mut NoHost, budget).unwrap();
                    if vm.pc == 100 {
                        assert!(vm.stack_bytes().is_empty());
                        observed_data_stack = true;
                    }
                    if step == Step::Halted {
                        halted = true;
                        break;
                    }
                    assert_eq!(step, Step::Yield);
                }
                assert!(halted);
                assert!(budget != 1 || observed_data_stack);
                assert_eq!(vm.stack_bytes(), value.to_le_bytes());
                let mut prefix = [0xa5; 8];
                let offset = usize::from(!equal);
                prefix[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                assert_eq!(vm.read(0, 8).unwrap(), prefix);
                assert_eq!(
                    vm.read(32, 4).unwrap(),
                    if equal { 0xa5a5_a5a5u32 } else { 0x1122_3344 }.to_le_bytes()
                );
            }
        }
    }
    let mut empty =
        kcs::Interpreter::new(kcs::Program::from_code(&[0x2d], vec![], 8).unwrap(), vec![])
            .unwrap();
    assert!(
        empty
            .run(&mut NoHost, 1)
            .unwrap_err()
            .message
            .contains("下溢")
    );
}

#[test]
fn string_expansion_and_argument_boundaries_match_native_words() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 1024], 64).unwrap(),
        vec![0; 64],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    for (id, value) in [(0, b"Z"), (7, b"A"), (16, b"H"), (u32::MAX, b"N")] {
        state.strings.insert(id, value.to_vec());
    }
    let mut host = kcs::CoreHost::new(&mut state);
    for (input, expected) in [
        ("$str+7;X", "AX"),
        ("$str-1;X", "NX"),
        ("$str0x10;X", "HX"),
        ("$str4294967303;X", "AX"),
        ("$str0x;X", "ZX"),
        ("$str7.2X", "$str7.2X"),
        ("$str100;X", "X"),
        ("$str7$str7", "AA"),
    ] {
        vm.write(0, &[input.as_bytes(), &[0]].concat()).unwrap();
        assert_eq!(
            host.call(278, &0u32.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read_string(0).unwrap(), expected.as_bytes());
    }
    for (input, words) in [
        ("cmd a(b)c(d) tail", vec!["cmd", "a", "(b)c", "(d)", "tail"]),
        ("cmd x1(2) tail", vec!["cmd", "x1(2)", "tail"]),
        ("cmd (:a b)tail end", vec!["cmd", "a btail", "end"]),
        ("cmd (:) tail", vec!["cmd"]),
        ("cmd \"a b\" a\tb", vec!["cmd", "\"a", "b\"", "a\tb"]),
    ] {
        vm.write(0, &[input.as_bytes(), &[0]].concat()).unwrap();
        for i in 0..=words.len() {
            let args = [512u32, 0, i as u32]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(
                host.call(519, &args, &mut vm).unwrap(),
                kcs::NativeReturn::Value((i < words.len()) as u32)
            );
            assert_eq!(
                vm.read_string(512).unwrap(),
                words.get(i).unwrap_or(&"").as_bytes()
            );
        }
    }
    vm.write(0, b"$str7\0").unwrap();
    vm.write(64, b"$\0").unwrap();
    vm.write(80, b"0x\0").unwrap();
    let args = [128u32, 0, 64, 80]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    host.call(45, &args, &mut vm).unwrap();
    assert_eq!(vm.read_string(128).unwrap(), b"0xstr7");
    host.call(
        277,
        &[99u32.to_le_bytes(), 0x8000_0000u32.to_le_bytes()].concat(),
        &mut vm,
    )
    .unwrap();
    assert_eq!(vm.read_string(0x8000_0000).unwrap(), b"");
}

#[test]
fn word_offsets_preserve_space_runs_raw_bytes_and_native_stack_results() {
    use kcs::{Host, NativeReturn};
    let source = b" \x82\xa0\tX  b \0";
    let mut state = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut state);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0xa5; 64], 0).unwrap(),
        vec![0xb6; 64],
    )
    .unwrap();
    for address in [0u32, 0x8000_0001] {
        vm.write(address, source).unwrap();
        for (index, expected) in [
            (0, 0),
            (1, 1),
            (2, 7),
            (3, 9),
            (4, u32::MAX),
            (u32::MAX, u32::MAX),
        ] {
            let args = [address.to_le_bytes(), index.to_le_bytes()].concat();
            assert_eq!(
                host.call(540, &args, &mut vm).unwrap(),
                NativeReturn::Value(expected)
            );
        }
        assert_eq!(vm.read(address, source.len()).unwrap(), source);
    }
    assert_eq!(
        host.call(
            540,
            &[u32::MAX.to_le_bytes(), 0u32.to_le_bytes()].concat(),
            &mut vm
        )
        .unwrap(),
        NativeReturn::Value(0)
    );
    vm.write(61, b"a b").unwrap();
    assert_eq!(
        host.call(
            540,
            &[61u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
            &mut vm
        )
        .unwrap(),
        NativeReturn::Value(2)
    );
    assert!(
        host.call(
            540,
            &[61u32.to_le_bytes(), 2u32.to_le_bytes()].concat(),
            &mut vm
        )
        .is_err()
    );

    let mut code = Vec::new();
    for value in [0x1234_5678, 32, 2] {
        ins(&mut code, 0x25, &[(3, value)]);
    }
    ins(&mut code, 0x2b, &[(3, 540), (3, 8)]);
    ins(&mut code, 0x26, &[(3, 0)]);
    let mut data = vec![0; 64];
    data[32..32 + source.len()].copy_from_slice(source);
    let program = kcs::Program::from_code(&code, data, 32).unwrap();
    for budget in [1, 4096] {
        let mut vm = kcs::Interpreter::new(program.clone(), vec![]).unwrap();
        while vm.run(&mut host, budget).unwrap() != Step::Halted {}
        assert_eq!(vm.read(0, 4).unwrap(), 7u32.to_le_bytes());
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
    }
}

#[test]
fn integer_sequences_preserve_evaluation_gates_ranges_and_native_results() {
    use kcs::{Host, NativeReturn};
    let cases: &[(&[u8], i32, &[i32], i32)] = &[
        (b"$0+=1", 0, &[], 7),
        (b"$0+=1", 1, &[8], 8),
        (b"[$0+=1-$0+=4]", 0, &[], 12),
        (b"[$0+=1-$0+=4]", -1, &[], 12),
        (b"[$0+=1-$0+=4]", 2, &[8, 9], 12),
        (b"[$0+=1,$0+=4]", 0, &[], 7),
        (b"[$0+=1,$0+=4]", 1, &[8], 8),
        (b"[$0+=1,$0+=4]", 2, &[8, 12], 12),
        (b"[1,,2,]", 8, &[1, 0, 2], 7),
        (b"[1,2", 8, &[1, 2], 7),
        (b"[2-4],ignored", 8, &[-2], 7),
        (b"[-3]", 8, &[0, 1, 2, 3], 7),
        (b"[]", 8, &[0], 7),
        (b"[3]", 8, &[], 7),
        (
            b"[2147483647-2147483647]",
            3,
            &[i32::MAX, i32::MIN, i32::MIN + 1],
            7,
        ),
    ];
    for &(source, maximum, expected, shared) in cases {
        let mut vm = kcs::Interpreter::new(
            kcs::Program::from_code(&[], vec![0xa5; 64], 0).unwrap(),
            [source, b"\0"].concat(),
        )
        .unwrap();
        let mut state = katze_system2::expression::State::new(1);
        state.shared[0] = 7;
        let mut host = kcs::CoreHost::new(&mut state);
        let args = [0x8000_0000u32, 0, maximum as u32]
            .map(u32::to_le_bytes)
            .concat();
        assert_eq!(
            host.call(539, &args, &mut vm).unwrap(),
            NativeReturn::Value(expected.len() as u32)
        );
        let bytes: Vec<_> = expected.iter().flat_map(|n| n.to_le_bytes()).collect();
        assert_eq!(vm.read(0, bytes.len()).unwrap(), bytes);
        assert_eq!(host.state.shared[0], shared);
        assert_eq!(vm.read(bytes.len() as u32, 1).unwrap(), [0xa5]);
    }

    let mut code = Vec::new();
    for value in [0x1234_5678, 0, 0x8000_0001, 3] {
        ins(&mut code, 0x25, &[(3, value)]);
    }
    ins(&mut code, 0x2b, &[(3, 539), (3, 12)]);
    ins(&mut code, 0x26, &[(3, 32)]);
    let program = kcs::Program::from_code(&code, b"[$0+=1,\\0+=2,@]\0".to_vec(), 32).unwrap();
    for budget in [1, 4096] {
        let mut vm = kcs::Interpreter::new(program.clone(), vec![0xb6; 16]).unwrap();
        let mut state = katze_system2::expression::State::new(1);
        state.shared[0] = 7;
        state.local[0] = -3;
        state.current = 13;
        let mut host = kcs::CoreHost::new(&mut state);
        while vm.run(&mut host, budget).unwrap() != Step::Halted {}
        assert_eq!(vm.read(32, 4).unwrap(), 3u32.to_le_bytes());
        assert_eq!(vm.stack_bytes(), 0x1234_5678u32.to_le_bytes());
        assert_eq!(
            vm.read(0x8000_0001, 12).unwrap(),
            [8i32, -1, 13].map(i32::to_le_bytes).concat()
        );
        assert_eq!(host.state.shared, [8]);
        assert_eq!(host.state.local[0], -1);
    }
}

#[test]
fn integer_sequences_read_live_delimiters_and_keep_partial_progress_on_errors() {
    use kcs::{Host, NativeReturn};
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0xa5; 768], 0).unwrap(),
        vec![0xb6; 768],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(1);
    state.shared[0] = 7;
    let mut host = kcs::CoreHost::new(&mut state);
    let args = |source: u32, destination: u32, maximum: u32| {
        [source, destination, maximum]
            .map(u32::to_le_bytes)
            .concat()
    };
    for source in [0, 0x8000_0000] {
        vm.write(source, b"[32,7,8]\0").unwrap();
        assert_eq!(
            host.call(539, &args(source, source + 3, 4), &mut vm)
                .unwrap(),
            NativeReturn::Value(2)
        );
        assert_eq!(vm.read(source, 3).unwrap(), b"[32");
        assert_eq!(
            vm.read(source + 3, 8).unwrap(),
            [32u32, 0].map(u32::to_le_bytes).concat()
        );
    }
    vm.write(764, b"[1,x").unwrap();
    assert_eq!(
        host.call(539, &args(764, 0, 1), &mut vm).unwrap(),
        NativeReturn::Value(1)
    );
    vm.write(767, b"1").unwrap();
    assert_eq!(
        host.call(539, &args(767, 0, 0), &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    vm.write(0x8000_0000, b"[$0+=1,$0+=4]\0").unwrap();
    for (source, destination) in [(768, 0), (0x8000_0000, 768)] {
        assert_eq!(
            host.call(539, &args(source, destination, 2), &mut vm)
                .unwrap(),
            NativeReturn::Value(0)
        );
        assert_eq!(host.state.shared[0], 7);
    }
    assert!(host.call(539, &args(0x8000_0000, 762, 2), &mut vm).is_err());
    assert_eq!(vm.read(762, 4).unwrap(), 8u32.to_le_bytes());
    assert_eq!(host.state.shared[0], 12);
    for (length, succeeds) in [(259, true), (260, false)] {
        let source = [b"[1,".as_slice(), vec![b' '; length].as_slice(), b"]\0"].concat();
        vm.write(0x8000_0000, &source).unwrap();
        let result = host.call(539, &args(0x8000_0000, 0, 8), &mut vm);
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(vm.read(0, 4).unwrap(), 1u32.to_le_bytes());
    }
}

#[test]
fn comma_names_expand_before_splitting_and_keep_the_destination_on_failure() {
    use kcs::{Host, NativeReturn};
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 768], 0).unwrap(),
        vec![0; 768],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut state);
    let args = |source: u32, target: u32| [source, target].map(u32::to_le_bytes).concat();
    for (source, name) in [
        (b"".as_slice(), b"".as_slice()),
        (b",,,", b""),
        (b",,a,,b,", b"a"),
        (b" a ,b", b" a "),
        (b"\"a,b\",c", b"\"a"),
        (b"(a,b),c", b"(a"),
        (b"\x82\xa0,b", b"\x82\xa0"),
        (b"\x82,b", b"\x82"),
    ] {
        vm.write(0, &[source, b"\0"].concat()).unwrap();
        vm.write(64, &[0xa5; 8]).unwrap();
        assert_eq!(
            host.call(598, &args(0, 64), &mut vm).unwrap(),
            NativeReturn::Value(u32::from(!name.is_empty()))
        );
        if name.is_empty() {
            assert_eq!(vm.read(64, 8).unwrap(), [0xa5; 8]);
        } else {
            assert_eq!(vm.read(64, name.len() + 1).unwrap(), [name, b"\0"].concat());
            assert_eq!(vm.read(65 + name.len() as u32, 1).unwrap(), [0xa5]);
        }
    }
    vm.write(0, b",head%s[G32],tail\0").unwrap();
    vm.write(32, b",other\0").unwrap();
    assert_eq!(
        host.call(598, &args(0, 1), &mut vm).unwrap(),
        NativeReturn::Value(1)
    );
    assert_eq!(vm.read(1, 5).unwrap(), b"head\0");
    let source = [vec![b','; 300], b"name\0".to_vec()].concat();
    vm.write(0x8000_0000, &source).unwrap();
    assert_eq!(
        host.call(598, &args(0x8000_0000, 64), &mut vm).unwrap(),
        NativeReturn::Value(1)
    );
    assert_eq!(vm.read(64, 5).unwrap(), b"name\0");
    for source in [
        [vec![b'x'; 255], vec![0]].concat(),
        [b"name,".to_vec(), b"x,".repeat(65), vec![0]].concat(),
    ] {
        vm.write(0x8000_0000, &source).unwrap();
        vm.write(64, &[0xa5; 8]).unwrap();
        assert!(host.call(598, &args(0x8000_0000, 64), &mut vm).is_err());
        assert_eq!(vm.read(64, 8).unwrap(), [0xa5; 8]);
    }
    vm.write(0, b",,\0").unwrap();
    assert_eq!(
        host.call(598, &args(0, u32::MAX), &mut vm).unwrap(),
        NativeReturn::Value(0)
    );
    vm.write(0, b"a\0").unwrap();
    vm.write(767, &[0xa5]).unwrap();
    assert!(host.call(598, &args(0, 767), &mut vm).is_err());
    assert_eq!(vm.read(767, 1).unwrap(), [0xa5]);

    let mut code = Vec::new();
    for value in [0xdead_beef, 0, 64] {
        ins(&mut code, 0x25, &[(3, value)]);
    }
    ins(&mut code, 0x2b, &[(3, 598), (3, 8)]);
    ins(&mut code, 0x26, &[(3, 32)]);
    let mut data = vec![0; 96];
    data[..12].copy_from_slice(b",,name,tail\0");
    let program = kcs::Program::from_code(&code, data, 32).unwrap();
    for budget in [1, 4096] {
        let mut vm = kcs::Interpreter::new(program.clone(), vec![]).unwrap();
        while vm.run(&mut host, budget).unwrap() != Step::Halted {}
        assert_eq!(vm.read(32, 4).unwrap(), 1u32.to_le_bytes());
        assert_eq!(vm.read_string(64).unwrap(), b"name");
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
    }
}

#[test]
fn formatted_strings_use_byte_addresses_and_recurse_through_string_fields() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 512], 256).unwrap(),
        vec![0; 512],
    )
    .unwrap();
    vm.write(0, &(-7i32).to_le_bytes()).unwrap();
    vm.write(4, &0x12abu32.to_le_bytes()).unwrap();
    vm.write(8, &1.25f32.to_bits().to_le_bytes()).unwrap();
    vm.write(16, b"N%d[G0]\0").unwrap();
    vm.write(512, &42u32.to_le_bytes()).unwrap();
    vm.write(0x8000_0000, &13u32.to_le_bytes()).unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    for (input, expected) in [
        (
            "%d[G0]|%08x[G4]|%f[G8]|%s[G16]|%02d[L0]|%d[H0]|%%",
            "-7|000012ab|1.250000|N-7|42|13|%",
        ),
        ("%04d[G0]", "-007"),
        ("%8.2f[G8]", "    1.25"),
        ("%3s[G16]", "N-7"),
        ("%bad[Z0]|%d[Gbad]|plain%", "%bad[Z0]|%d[Gbad]|plain%"),
    ] {
        vm.write(100, &[input.as_bytes(), &[0]].concat()).unwrap();
        let args = [0x8000_0020u32.to_le_bytes(), 100u32.to_le_bytes()].concat();
        assert_eq!(
            host.call(14, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(0x8000_0020)
        );
        assert_eq!(vm.read_string(0x8000_0020).unwrap(), expected.as_bytes());
    }
    for (bits, expected) in [
        (0x7fc0_1234u32, "nan"),
        (0xffc0_1234, "-nan(ind)"),
        (0x7f80_0000, "inf"),
        (0xff80_0000, "-inf"),
        (0x8000_0000, "-0.000000"),
    ] {
        vm.write(8, &bits.to_le_bytes()).unwrap();
        vm.write(100, b"%f[G8]\0").unwrap();
        host.call(
            14,
            &[300u32.to_le_bytes(), 100u32.to_le_bytes()].concat(),
            &mut vm,
        )
        .unwrap();
        assert_eq!(vm.read_string(300).unwrap(), expected.as_bytes());
    }
    vm.write(100, b"%s[G100]\0").unwrap();
    assert!(
        host.call(
            14,
            &[300u32.to_le_bytes(), 100u32.to_le_bytes()].concat(),
            &mut vm
        )
        .is_err()
    );
}

#[test]
fn raw_and_formatted_string_comparisons_keep_opposite_orders_and_signed_byte_rules() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 512], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    let args = [64u32.to_le_bytes(), 128u32.to_le_bytes()].concat();
    vm.write(64, b"\x80\0").unwrap();
    vm.write(128, b"a\0").unwrap();
    for (index, expected) in [(19, 225i32), (20, -225), (21, -31), (22, 31)] {
        assert_eq!(
            host.call(index, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected as u32)
        );
    }
    vm.write(64, b"a\0").unwrap();
    vm.write(128, b"A\0").unwrap();
    for (index, expected) in [(19, -32i32), (20, 32), (21, 0), (22, 0)] {
        assert_eq!(
            host.call(index, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected as u32)
        );
    }
    vm.write(0, &42u32.to_le_bytes()).unwrap();
    vm.write(64, b"%d[G0]\0").unwrap();
    vm.write(128, b"42\0").unwrap();
    assert_eq!(
        host.call(20, &args, &mut vm).unwrap(),
        kcs::NativeReturn::Value(0)
    );
    assert_eq!(
        host.call(18, &64u32.to_le_bytes(), &mut vm).unwrap(),
        kcs::NativeReturn::Value(2)
    );
    vm.write(64, b"%c[G4]suffix\0").unwrap();
    assert_eq!(
        host.call(18, &64u32.to_le_bytes(), &mut vm).unwrap(),
        kcs::NativeReturn::Value(0)
    );
    vm.write(256, &[0xcc; 16]).unwrap();
    host.call(
        14,
        &[256u32.to_le_bytes(), 64u32.to_le_bytes()].concat(),
        &mut vm,
    )
    .unwrap();
    assert_eq!(vm.read(256, 3).unwrap(), &[0, 0, 0xcc]);
}

#[test]
fn string_copy_and_append_keep_double_termination_and_forward_dword_overlap() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 512], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    vm.write(0, &42u32.to_le_bytes()).unwrap();
    vm.write(64, b"%d[G0]\0").unwrap();
    let args = [256u32.to_le_bytes(), 64u32.to_le_bytes()].concat();
    assert_eq!(
        host.call(13, &args, &mut vm).unwrap(),
        kcs::NativeReturn::Value(256)
    );
    assert_eq!(vm.read(256, 8).unwrap(), b"%d[G0]\0\0");
    host.call(16, &args, &mut vm).unwrap();
    assert_eq!(vm.read(256, 10).unwrap(), b"%d[G0]42\0\0");
    vm.write(64, b"abcd\0\0\0\0\0\0").unwrap();
    assert_eq!(
        host.call(
            15,
            &[64u32.to_le_bytes(), 64u32.to_le_bytes()].concat(),
            &mut vm
        )
        .unwrap(),
        kcs::NativeReturn::Value(64)
    );
    assert_eq!(vm.read(64, 11).unwrap(), b"abcdabcda\0\0");
    assert!(
        host.call(
            13,
            &[65u32.to_le_bytes(), 64u32.to_le_bytes()].concat(),
            &mut vm
        )
        .is_err()
    );
    assert_eq!(
        host.call(6, &u32::MAX.to_le_bytes(), &mut vm).unwrap(),
        kcs::NativeReturn::Value(u32::MAX)
    );
    for index in [2, 3] {
        assert_eq!(
            host.call(index, &[], &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
    }
}

#[test]
fn core_character_helpers_use_valid_baseline_pairs_and_distinguish_byte_and_character_limits() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 512], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    for (source, byte_length, character_length, pair) in [
        (b"A\x82\xa0BC".as_slice(), 1, 4, 1),
        (b"A\xf0\x40B", 2, 3, 0),
        (b"A\x81\x7fB", 2, 3, 0),
    ] {
        vm.write(64, &[source, &[0]].concat()).unwrap();
        assert_eq!(
            host.call(40, &65u32.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Value(pair)
        );
        assert_eq!(
            host.call(47, &64u32.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Value(4)
        );
        let args = [64u32.to_le_bytes(), 3u32.to_le_bytes()].concat();
        assert_eq!(
            host.call(23, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(byte_length)
        );
        assert_eq!(vm.read(64 + byte_length, 2).unwrap(), &[0, 0]);
        vm.write(64, &[source, &[0]].concat()).unwrap();
        assert_eq!(
            host.call(24, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(character_length)
        );
        assert_eq!(vm.read(64 + character_length, 2).unwrap(), &[0, 0]);
    }
    vm.write(0, &42u32.to_le_bytes()).unwrap();
    vm.write(64, b"x%d[G0]\0").unwrap();
    assert_eq!(
        host.call(48, &64u32.to_le_bytes(), &mut vm).unwrap(),
        kcs::NativeReturn::Value(3)
    );
}

#[test]
fn core_integer_conversion_expands_formats_and_uses_engine_prefix_rules() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 512], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    vm.write(0, &42u32.to_le_bytes()).unwrap();
    for (source, expected) in [
        ("%d[G0]tail", 42),
        (" 12", 0),
        ("12.5", 0),
        ("12e4", 12),
        ("-0x10", u32::MAX - 15),
        ("0x1.5", 1),
        ("4294967297", 1),
    ] {
        vm.write(64, &[source.as_bytes(), &[0]].concat()).unwrap();
        assert_eq!(
            host.call(25, &64u32.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected)
        );
    }
}

#[test]
fn percentage_strings_scale_in_place_with_unsigned_bases_and_single_precision() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 256], 64).unwrap(),
        vec![0; 256],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut state);
    for (source, base, expected) in [
        ("50%", 3, "1"),
        ("-50%", 3, "-1"),
        ("+33%", 1920, "633"),
        ("0x10%", 100, "16"),
        ("0x%", 100, "0"),
        ("0x1.5%", 100, "1"),
        ("12tail%", 100, "12"),
        ("100%%", 100, "100"),
        ("16777217%", 100, "16777216"),
        ("4294967297%", 1920, "19"),
        ("99%", u32::MAX, "-2147483648"),
        ("", 100, ""),
        ("%", 100, "%"),
        (" 12%", 100, " 12%"),
        ("12.5%", 100, "12.5%"),
        ("12%tail", 100, "12%tail"),
    ] {
        for address in [64u32, 0x8000_0040] {
            let mut original = [0x55; 64];
            original[..source.len()].copy_from_slice(source.as_bytes());
            original[source.len()] = 0;
            vm.write(address, &original).unwrap();
            let arguments = [address.to_le_bytes(), base.to_le_bytes()].concat();
            assert_eq!(
                host.call(46, &arguments, &mut vm).unwrap(),
                kcs::NativeReturn::Continue
            );
            let mut actual = original;
            actual[..expected.len()].copy_from_slice(expected.as_bytes());
            actual[expected.len()] = 0;
            assert_eq!(&*vm.read(address, 64).unwrap(), actual);
        }
    }
    assert_eq!(
        host.call(
            46,
            &[0x8000_0100u32.to_le_bytes(), 100u32.to_le_bytes()].concat(),
            &mut vm
        )
        .unwrap(),
        kcs::NativeReturn::Continue
    );
    assert!(host.call(46, &[0; 4], &mut vm).is_err());
}

#[test]
fn core_scalar_math_keeps_table_mapping_sse_angle_order_and_pow_nan_signs() {
    use kcs::Host;
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&[], vec![], 64).unwrap(), vec![]).unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    for (index, arguments, expected) in [
        (50, vec![0], 0),
        (51, vec![0], 1.0f32.to_bits()),
        (56, vec![(-1.25f32).to_bits()], (-1.0f32).to_bits()),
        (57, vec![0xff80_1234], 0x7fc0_1234),
        (58, vec![(-1.25f32).to_bits()], (-2.0f32).to_bits()),
        (
            59,
            vec![7.0f32.to_bits(), 2.0f32.to_bits()],
            1.0f32.to_bits(),
        ),
        (
            61,
            vec![2.0f32.to_bits(), 3.0f32.to_bits()],
            8.0f32.to_bits(),
        ),
        (61, vec![0xff80_1234, (-3.0f32).to_bits()], 0x7fc0_1234),
        (61, vec![0xffc0_1234, 2.0f32.to_bits()], 0xffc0_1234),
        (62, vec![f32::MAX.to_bits()], f32::INFINITY.to_bits()),
        (63, vec![f32::MAX.to_bits()], f32::INFINITY.to_bits()),
        (64, vec![i32::MIN as u32], i32::MIN as u32),
        (397, vec![0x8000_0000, 0], 0x8000_0000),
        (
            397,
            vec![1f32.to_bits(), 0],
            std::f32::consts::FRAC_PI_2.to_bits(),
        ),
        (
            397,
            vec![0, (-1f32).to_bits()],
            std::f32::consts::PI.to_bits(),
        ),
        (397, vec![0xff80_1234, 0], 0xffc0_1234),
        (397, vec![0x7fc0_1234, 0xff80_5678], 0xffc0_5678),
        (
            318,
            vec![2., 10., 4., 8., 3.]
                .into_iter()
                .map(f32::to_bits)
                .collect(),
            2.0f32.to_bits(),
        ),
        (
            318,
            vec![2., 10., 4., 8., 6.]
                .into_iter()
                .map(f32::to_bits)
                .collect(),
            6.0f32.to_bits(),
        ),
        (
            318,
            vec![2., 10., 4., 8., 9.]
                .into_iter()
                .map(f32::to_bits)
                .collect(),
            10.0f32.to_bits(),
        ),
        (
            318,
            vec![2., 10., 4., 4., 4.]
                .into_iter()
                .map(f32::to_bits)
                .collect(),
            2.0f32.to_bits(),
        ),
        (319, vec![(-1.5f32).to_bits()], u32::MAX),
        (319, vec![0.5f32.to_bits()], 1),
        (319, vec![f32::INFINITY.to_bits()], i32::MIN as u32),
    ] {
        let arguments = arguments
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            host.call(index, &arguments, &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected)
        );
    }
    assert_eq!(
        host.call(65, &[], &mut vm).unwrap(),
        kcs::NativeReturn::YieldVoid
    );
    host.task_origin = u32::MAX;
    host.task_time = 2;
    assert_eq!(
        host.call(5, &[], &mut vm).unwrap(),
        kcs::NativeReturn::Value(3)
    );
    assert_eq!((host.task_origin, host.task_time), (u32::MAX, 2));
}

#[test]
fn script_replacements_chain_after_formatting_and_keep_intermediate_storage() {
    use kcs::Host;

    let mut state = katze_system2::expression::State::new(0);
    let mut host = kcs::CoreHost::new(&mut state);
    for external in [0, 0x8000_0000] {
        let mut vm = kcs::Interpreter::new(
            kcs::Program::from_code(&[], vec![0x55; 256], 0).unwrap(),
            vec![0x55; 256],
        )
        .unwrap();
        vm.write(32, &12u32.to_le_bytes()).unwrap();
        vm.write(128, b"a%u[G32]\0").unwrap();
        let args: Vec<_> = [external + 64, 128]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        host.script_replacements.clear();
        assert_eq!(
            host.call(1002, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read(external + 64, 5).unwrap(), b"a12\0U");
        host.script_replacements.extend([
            (b"cc".to_vec(), Vec::new()),
            (b"b".to_vec(), b"c".to_vec()),
            (b"a".to_vec(), b"bbbb".to_vec()),
        ]);
        host.call(1002, &args, &mut vm).unwrap();
        assert_eq!(vm.read(external + 64, 8).unwrap(), b"12\0c12\0U");
        host.script_replacements.clear();
        host.script_replacements
            .insert(b"a\0unused".to_vec(), b"q\0unused".to_vec());
        let args: Vec<_> = [128u32, 128]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        host.call(1002, &args, &mut vm).unwrap();
        assert_eq!(vm.read_string(128).unwrap(), b"q12");
        host.script_replacements.clear();
        host.script_replacements.insert(vec![0x82], b"X".to_vec());
        vm.write(128, b"\x82\xa0\0").unwrap();
        host.call(1002, &args, &mut vm).unwrap();
        assert_eq!(vm.read_string(128).unwrap(), b"X\xa0");
        for (find, replacement, source, expected) in [
            (
                b"a".as_slice(),
                b"b".as_slice(),
                b"a".as_slice(),
                b"b\0\0U".as_slice(),
            ),
            (b"a", b"b", b"A", b"A\0UU"),
            (b"a", b"b", b"", b"\0\0UU"),
            (b"aa", b"bbbb", b"aaa", b"bbbba\0U"),
        ] {
            host.script_replacements.clear();
            host.script_replacements
                .insert(find.to_vec(), replacement.to_vec());
            vm.write(160, &[find, &[0]].concat()).unwrap();
            vm.write(176, &[replacement, &[0]].concat()).unwrap();
            for native in [45, 1002] {
                vm.write(128, &[source, &[0]].concat()).unwrap();
                vm.write(external + 64, &[0x55; 8]).unwrap();
                let mut words = vec![external + 64, 128];
                if native == 45 {
                    words.extend([160, 176]);
                }
                let args: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
                host.call(native, &args, &mut vm).unwrap();
                assert_eq!(vm.read(external + 64, expected.len()).unwrap(), expected);
            }
        }
    }
}

#[test]
fn parenthesized_words_keep_groups_and_observe_overlapping_writes() {
    use kcs::Host;

    for external in [0, 0x8000_0000] {
        for (source, separators, destination, start, expected) in [
            (
                b"a(x y)z".as_slice(),
                b" ".as_slice(),
                32,
                32,
                b"a\0(x y)z\0\0".as_slice(),
            ),
            (b"1(x y)z", b" ", 32, 32, b"1(x y)z\0\0"),
            (b"a(:x y)z", b" ", 32, 32, b"a\0x yz\0\0"),
            (b"a(a(b)c)z", b" ", 32, 32, b"a\0(a(b)c)z\0\0"),
            (b"\x82\xa0 a", b"\xa0", 32, 32, b"\x82\0 a\0\0"),
            (b"a(x y)z", b" ", 128, 128, b"a\0\0\0y)z\0"),
            (b"a b", b" ", 192, 192, b"a b\0\0"),
            (b"(:", b" ", 32, 31, b"\0\0:\0\0"),
        ] {
            let mut vm = kcs::Interpreter::new(
                kcs::Program::from_code(&[], vec![0; 256], 0).unwrap(),
                vec![0; 256],
            )
            .unwrap();
            vm.write(external + 128, &[source, &[0]].concat()).unwrap();
            vm.write(external + 192, &[separators, &[0]].concat())
                .unwrap();
            let mut state = katze_system2::expression::State::new(0);
            let mut host = kcs::CoreHost::new(&mut state);
            let args: Vec<_> = [external + destination, external + 128, external + 192]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert_eq!(
                host.call(43, &args, &mut vm).unwrap(),
                kcs::NativeReturn::Continue
            );
            assert_eq!(vm.read(external + start, expected.len()).unwrap(), expected);
        }
    }
    for invalid in 0..3 {
        let mut vm = kcs::Interpreter::new(
            kcs::Program::from_code(&[], vec![0x55; 256], 0).unwrap(),
            vec![],
        )
        .unwrap();
        vm.write(128, b"a b\0").unwrap();
        vm.write(192, b" \0").unwrap();
        let before = vm.read(0, 256).unwrap().to_vec();
        let mut state = katze_system2::expression::State::new(0);
        let mut host = kcs::CoreHost::new(&mut state);
        let mut args = [32, 128, 192];
        args[invalid] = u32::MAX;
        let args: Vec<_> = args.into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(
            host.call(43, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read(0, 256).unwrap(), before);
    }
}

#[test]
fn byte_truncation_and_zero_separated_word_helpers_keep_boundaries() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 1024], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    for (limit, length) in [(0, 0), (1, 0), (2, 1), (3, 1), (4, 3), (5, 4), (6, 5)] {
        vm.write(100, b"A\x82\xa0BC\0").unwrap();
        let args = [100u32.to_le_bytes(), u32::to_le_bytes(limit)].concat();
        assert_eq!(
            host.call(23, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value(length)
        );
        assert_eq!(vm.read_string(100).unwrap().len(), length as usize);
    }
    vm.write(100, b" ,a,, b c,\0").unwrap();
    vm.write(200, b", \0").unwrap();
    let args = [300u32, 100, 200]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        host.call(41, &args, &mut vm).unwrap(),
        kcs::NativeReturn::Continue
    );
    assert_eq!(vm.read(300, 8).unwrap(), b"a\0b\0c\0\0\0");
    for (index, word) in [(-1i32, "a"), (0, "a"), (1, "b"), (2, "c"), (3, "")] {
        let args = [400u32, 300, index as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            host.call(42, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Value((!word.is_empty()) as u32)
        );
        assert_eq!(vm.read_string(400).unwrap(), word.as_bytes());
    }
    for (first, last, expected) in [(0u32, u32::MAX, "a b c "), (1, 1, "b ")] {
        let args = [400u32, 300, first, last]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        host.call(44, &args, &mut vm).unwrap();
        assert_eq!(vm.read_string(400).unwrap(), expected.as_bytes());
    }
    vm.write(100, b"alpha,beta\tlast\0").unwrap();
    vm.write(200, b",\\t\0").unwrap();
    let args = [400u32, 100, 200, 20]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        host.call(38, &args, &mut vm).unwrap(),
        kcs::NativeReturn::Value(5)
    );
    assert_eq!(vm.read_string(400).unwrap(), b"alpha");
    for (source, separators, expected) in [
        (
            b"\x82,a".as_slice(),
            b",".as_slice(),
            b"\x82\0a\0\0".as_slice(),
        ),
        (b"\xf0\xa0a", b"\xa0", b"\xf0\0a\0\0"),
        (b"\x82\xa0a", b"\xa0", b"\x82\xa0a\0\0"),
    ] {
        vm.write(100, &[source, &[0]].concat()).unwrap();
        vm.write(200, &[separators, &[0]].concat()).unwrap();
        let args: Vec<_> = [400u32, 100, 200]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        host.call(41, &args, &mut vm).unwrap();
        assert_eq!(vm.read(400, expected.len()).unwrap(), expected);
    }
}

#[test]
fn text_float_conversion_consumes_a_prefix_and_rounds_through_double() {
    use kcs::Host;
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 256], 64).unwrap(),
        vec![],
    )
    .unwrap();
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    for (input, expected) in [
        ("", 0),
        ("abc", 0),
        ("12.5tail", 12.5f32.to_bits()),
        ("  -2.75e2x", (-275.0f32).to_bits()),
        ("\u{b}\u{c}\r\n\t 3.5", 3.5f32.to_bits()),
        ("+.5", 0.5f32.to_bits()),
        ("1e+", 1.0f32.to_bits()),
        ("1.e-2", 0.01f32.to_bits()),
        ("0x10", 16.0f32.to_bits()),
        ("0x1p2", 4.0f32.to_bits()),
        ("0x.8p-148", 1),
        ("inf", 0x7f80_0000),
        ("-INF", 0xff80_0000),
        ("nan", 0x7fff_ffff),
        ("-nan(ind)", 0xffff_ffff),
        ("1e999", 0x7f80_0000),
        ("-1e-999", 0x8000_0000),
        ("1.0000000596046448", 1.0f32.to_bits()),
        ("1,5", 1.0f32.to_bits()),
        ("%f[G128]", 1.25f32.to_bits()),
    ] {
        vm.write(128, &1.25f32.to_bits().to_le_bytes()).unwrap();
        vm.write(0, &[input.as_bytes(), &[0]].concat()).unwrap();
        assert_eq!(
            host.call(26, &0u32.to_le_bytes(), &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected),
            "{input}"
        );
    }
    vm.write(0, b"value=%f[G128]%%\0").unwrap();
    assert_eq!(
        host.call(1, &0u32.to_le_bytes(), &mut vm).unwrap(),
        kcs::NativeReturn::Continue
    );
    assert_eq!(host.debug_messages, [b"value=1.250000%".to_vec()]);
}

#[test]
fn ordinary_random_natives_advance_separate_integer_and_float_streams() {
    use kcs::Host;
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    host.random = kcs::ScriptRandom::seeded(0);
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&[], vec![], 64).unwrap(), vec![]).unwrap();
    for (index, expected) in [
        (34, 0x3de3_ae3e),
        (33, 0x1c75_c7c9),
        (33, 0xda96_49c5),
        (34, 0x3f5a_964a),
        (33, 0x84e2_43e5),
        (33, 0xa75c_2a2f),
    ] {
        assert_eq!(
            host.call(index, &[0xff], &mut vm).unwrap(),
            kcs::NativeReturn::Value(expected)
        );
    }
}

#[test]
fn grouped_integer_format_keeps_sign_rules_minimum_integer_and_double_zero() {
    use kcs::Host;
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 64], 64).unwrap(),
        vec![0; 64],
    )
    .unwrap();
    for (value, sign, expected) in [
        (0, 1, "0"),
        (123, 0, "123"),
        (1234567, u32::MAX, "+1,234,567"),
        ((-1234567i32) as u32, 1, "-1,234,567"),
        (i32::MIN as u32, 0, "-.,/,),,(-,*,("),
    ] {
        let args = [0x8000_0004u32, value, sign]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            host.call(37, &args, &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read_string(0x8000_0004).unwrap(), expected.as_bytes());
        assert_eq!(
            vm.read(0x8000_0004 + expected.len() as u32, 2).unwrap(),
            [0, 0]
        );
    }
}

#[test]
fn memory_copy_keeps_forward_cpu_blocks_and_crt_overlap_results() {
    use kcs::{Host, MemoryCopyMode};
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 64], 64).unwrap(),
        vec![],
    )
    .unwrap();
    for (mode, expected) in [
        (
            MemoryCopyMode::Mmx,
            [0, 1, 2, 3, 4, 5, 6, 7, 7, 9, 10, 11, 12, 13, 14, 15].as_slice(),
        ),
        (
            MemoryCopyMode::Dword,
            &[0, 1, 2, 3, 3, 5, 6, 7, 7, 9, 10, 11, 11, 13, 14, 15],
        ),
        (
            MemoryCopyMode::Crt,
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        ),
        (MemoryCopyMode::Mmx, &[0, 1, 2, 3, 3, 5, 6, 7, 7, 9, 10, 11]),
        (
            MemoryCopyMode::Mmx,
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
        ),
    ] {
        vm.write(0, &(0u8..64).collect::<Vec<_>>()).unwrap();
        host.memory_copy_mode = mode;
        let arguments: Vec<_> = [1, 0, expected.len() as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(
            host.call(35, &arguments, &mut vm).unwrap(),
            kcs::NativeReturn::Continue
        );
        assert_eq!(vm.read(1, expected.len()).unwrap(), expected);
    }
}

#[test]
fn coordinate_queries_preserve_raw_values_and_write_in_argument_order() {
    use kcs::Host;
    let mut state = katze_system2::expression::State::new(8);
    let mut host = kcs::CoreHost::new(&mut state);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; 32], 64).unwrap(),
        vec![0; 32],
    )
    .unwrap();
    for native in [8, 149] {
        host.surface_size = [0x1122_3344, 0xaabb_ccdd];
        host.pointer_position = [0x1122_3344, 0xaabb_ccddu32 as i32];
        for (addresses, target, expected) in [
            ([1, 2], 1, [0x44, 0xdd, 0xcc, 0xbb, 0xaa].as_slice()),
            (
                [0x8000_0000, 0x8000_0000],
                0x8000_0000,
                &[0xdd, 0xcc, 0xbb, 0xaa],
            ),
        ] {
            let arguments: Vec<_> = addresses.into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(
                host.call(native, &arguments, &mut vm).unwrap(),
                kcs::NativeReturn::Continue
            );
            assert_eq!(vm.read(target, expected.len()).unwrap(), expected);
        }
        host.surface_size = [u32::MAX, 0x8000_0000];
        host.pointer_position = [-1, i32::MIN];
        let arguments: Vec<_> = [0, 0x8000_0004]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        host.call(native, &arguments, &mut vm).unwrap();
        assert_eq!(vm.read(0, 4).unwrap(), u32::MAX.to_le_bytes());
        assert_eq!(
            vm.read(0x8000_0004, 4).unwrap(),
            0x8000_0000u32.to_le_bytes()
        );
    }
}
