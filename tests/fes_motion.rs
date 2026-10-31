use katze_system2::{Step, expression::State, fes, kcs};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn image_subcommand_arguments_reach_motion_parameters_and_write_back_position() {
    let mut image = fes::ImageState::default();
    image.base.position = [10.5, -3.25];
    image.base.base_point = [1.25, 2.5];
    image.base.priority = 42;
    image.base.parent_priority = 22;
    image.base.color = 0x1234_5678;
    let mut state = State::new(16);
    let mut fes = fes::Interpreter::new(
        fes::Program::from_text(b"#SUBCOMMAND\noneshot move motion px[0] : [0]\n#START\nexit 0\n")
            .unwrap(),
    )
    .unwrap();
    let table = fes.subcommands(&state);
    assert!(table.diagnostics.is_empty());
    let arguments = fes
        .subcommand_arguments(
            &table.bindings[b"move".as_slice()],
            &[b"25".to_vec()],
            &image.base.subcommand_context(),
            &mut state,
        )
        .unwrap();
    assert_eq!(arguments, [14.5]);
    let mut code = vec![];
    ins(&mut code, 1, &[(0x14, 0x8000_0018), (0x14, 0x8000_009c)]);
    ins(&mut code, 0x15, &[(4, 0x8000_0018), (0x44, 0)]);
    code.push(0x2d);
    ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    let motion_state = kcs::MotionState::with_parameters(&arguments).unwrap();
    let mut task = kcs::MotionTask::new(
        kcs::Program::from_code(&code, vec![], 64).unwrap(),
        motion_state,
        0,
    )
    .unwrap();
    task.set_properties(&image.base.motion_properties())
        .unwrap();
    assert!(matches!(
        task.update(0, &mut kcs::MotionHost::default(), 100)
            .unwrap(),
        Some(Step::Event(_))
    ));
    image
        .base
        .apply_motion_properties(&task.state().unwrap().properties);
    assert_eq!(image.base.position, [25.0, -3.25]);
    assert_eq!(image.base.priority, 42);
    assert_eq!(image.base.parent_priority, 22);
    assert_eq!(image.base.color, 0x1234_5678);
    assert_eq!(image.base.subcommand_context().base, [1.25, 2.5, 42.0]);
}

#[test]
fn cimage_motion_keeps_zero_signs_float_priority_rounding_and_alpha_only_color() {
    let mut image = fes::CImageState {
        position: [-0.0, 0.0],
        priority: i32::MIN,
        parent_priority: 123,
        color: 0x1234_5678,
        blend: 511,
        ..Default::default()
    };
    let mut properties = image.motion_properties();
    assert_eq!(properties[5], 0x4f00_0000);
    assert_eq!(properties[9..25], [0; 16]);
    assert_eq!(properties[28], 0xff00_0000);
    properties[6] = 0.0f32.to_bits();
    properties[28] = 0x7f12_3456;
    properties[35] = u32::MAX;
    image.apply_motion_properties(&properties);
    assert_eq!(image.position[0].to_bits(), (-0.0f32).to_bits());
    assert_eq!(image.priority, i32::MIN);
    assert_eq!(image.parent_priority, 123);
    assert_eq!(image.color, 0x1234_5678);
    assert_eq!(image.blend, 127);
    assert_eq!(image.display, -1);
}
