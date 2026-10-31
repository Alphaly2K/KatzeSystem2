use katze_system2::{
    Step,
    kcs::{self, Host, NativeReturn, ParticleHost, ParticleVertex},
};

fn call(
    host: &mut ParticleHost,
    vm: &mut kcs::Interpreter,
    index: u32,
    values: &[u32],
) -> NativeReturn {
    host.call(
        index,
        &values
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}

fn vm(size: usize) -> kcs::Interpreter {
    kcs::Interpreter::new(
        kcs::Program::from_code(&[], vec![0; size], 64).unwrap(),
        vec![],
    )
    .unwrap()
}

#[test]
fn particle_math_keeps_x86_nan_bits_rounding_and_time_units() {
    let mut host = ParticleHost {
        frame_rate: 30,
        ..Default::default()
    };
    let mut vm = vm(0);
    for (native, args, expected) in [
        (6, vec![f32::INFINITY.to_bits()], 0xffc0_0000),
        (7, vec![0xffc1_2345], 0xffc1_2345),
        (14, vec![0xff81_2345], 0x7fc1_2345),
        (16, vec![0, 0], 0xffc0_0000),
        (17, vec![(-1.0f32).to_bits()], 0xffc0_0000),
        (18, vec![f32::NAN.to_bits(), 0], 1.0f32.to_bits()),
        (19, vec![90.0f32.to_bits()], 0x3fc9_0fdb),
        (21, vec![0x8000_0000], 0x8000_0000),
        (42, vec![(-1.5f32).to_bits()], (-1i32) as u32),
        (42, vec![(-0.5f32).to_bits()], 0),
        (
            43,
            vec![1.0f32.to_bits(), 4.0f32.to_bits()],
            1.0f32.to_bits(),
        ),
        (3, vec![7], 3),
        (3, vec![1], 1),
        (3, vec![0], 0),
        (3, vec![u32::MAX], u32::MAX),
    ] {
        assert_eq!(
            call(&mut host, &mut vm, native, &args),
            NativeReturn::Value(expected)
        );
    }
    assert!(host.call(14, &[], &mut vm).is_err());
    assert!(host.call(38, &[], &mut vm).is_err());
}

#[test]
fn particle_quad_storage_and_snapshots_preserve_vertex_fields_and_rotation_direction() {
    let vertex = ParticleVertex {
        position: [1.0, 0.0, 0.0],
        rhw: 2.0,
        color: 0x1234_5678,
        uv: [0.25, 0.75],
    };
    let quad = [vertex; 4];
    let mut host = ParticleHost {
        quads: vec![quad],
        saved_quads: vec![quad],
        ..Default::default()
    };
    let mut vm = vm(256);
    assert_eq!(call(&mut host, &mut vm, 30, &[0]), NativeReturn::Continue);
    call(
        &mut host,
        &mut vm,
        24,
        &[0, std::f32::consts::FRAC_PI_2.to_bits()],
    );
    call(&mut host, &mut vm, 31, &[0]);
    let rotated = host.quads[0][0];
    assert!(rotated.position[0].abs() < 0.000001);
    assert_eq!(rotated.position[1..], [-1.0, 0.0]);
    assert_eq!(
        (rotated.rhw, rotated.color, rotated.uv),
        (2.0, vertex.color, vertex.uv)
    );
    call(&mut host, &mut vm, 45, &[]);
    let saved = host.quads.clone();
    host.quads[0][0].color = 0;
    call(&mut host, &mut vm, 44, &[]);
    assert_eq!(host.quads, saved);
    assert!(host.call(30, &250u32.to_le_bytes(), &mut vm).is_err());
}

#[test]
fn particle_offsets_uv_flags_and_parameters_follow_distinct_native_contracts() {
    let mut host = ParticleHost {
        surface_size: [10, 20],
        parameters: std::array::from_fn(|i| 0x1234_0000 + i as u32),
        ..Default::default()
    };
    let mut vm = vm(256);
    call(
        &mut host,
        &mut vm,
        27,
        &[1.25f32.to_bits(), (-2.5f32).to_bits(), 3.75f32.to_bits()],
    );
    call(&mut host, &mut vm, 28, &[0, 4, 8]);
    assert_eq!(
        vm.read(0, 12).unwrap(),
        [1.25f32, -2.5, 3.75]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(host.offsets[0][3], 0.0);
    call(
        &mut host,
        &mut vm,
        29,
        &[1.5f32.to_bits(), 999.0f32.to_bits(), 8.5f32.to_bits()],
    );
    assert_eq!(
        host.quads[0].map(|v| v.uv),
        [[0.1, 0.05], [0.8, 0.05], [0.1, 0.4], [0.8, 0.4]]
    );
    call(&mut host, &mut vm, 39, &[u32::MAX, 99]);
    assert_eq!(host.quad_flags, [1]);
    call(&mut host, &mut vm, 35, &[128]);
    assert_eq!(vm.read(128, 4).unwrap(), 0x1234_0000u32.to_le_bytes());
    assert_eq!(vm.read(252, 4).unwrap(), 0x1234_001fu32.to_le_bytes());
}

#[test]
fn particle_end_updates_task_state_and_yields_status_four() {
    let mut code = vec![0x2b, 3];
    code.extend(32u32.to_le_bytes());
    code.push(3);
    code.extend(0u32.to_le_bytes());
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&code, vec![], 64).unwrap(), vec![]).unwrap();
    let mut host = ParticleHost::default();
    assert_eq!(
        vm.run(&mut host, 10).unwrap(),
        Step::Event(kcs::Event {
            native_index: 32,
            status: 4
        })
    );
    assert!(!host.active);
}

#[test]
fn script_random_keeps_independent_streams_across_mt_state_updates() {
    let mut random = kcs::ScriptRandom::seeded(0);
    for step in 0..4000 {
        let value = if step % 5 <= 1 {
            random.next_integer()
        } else {
            random.next_float().to_bits()
        };
        let expected = match step {
            0 => Some(0x1c75_c7c9),
            1 => Some(0xda96_49c5),
            2 => Some(0x3de3_ae3e),
            3 => Some(0x3f5a_964a),
            4 => Some(0x3f04_e244),
            1555 => Some(0xa426_aad0),
            1556 => Some(0x35a2_b6df),
            1560 => Some(0x50c6_b262),
            3120 => Some(0x1ae6_144d),
            3999 => Some(0x3f4a_d165),
            _ => None,
        };
        if let Some(expected) = expected {
            assert_eq!(value, expected, "step {step}");
        }
    }
}
