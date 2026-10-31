use katze_system2::{Step, expression::State, fes, kcs};
use std::io::Write;

fn archived_motion(code: &[u8]) -> Vec<u8> {
    let mut container = b"KCS\0".to_vec();
    for value in [
        2,
        0x20000,
        52,
        code.len() as u32,
        52 + code.len() as u32,
        0,
        0,
        0,
        64,
        52 + code.len() as u32,
        0,
        code.len() as u32,
    ] {
        container.extend(value.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(code).unwrap();
    container.extend(encoder.finish().unwrap());
    let mut archive = b"KIF\0".to_vec();
    archive.extend(1u32.to_le_bytes());
    let mut name = [0; 64];
    name[..10].copy_from_slice(b"motion.kcs");
    archive.extend(name);
    archive.extend(80u32.to_le_bytes());
    archive.extend((container.len() as u32).to_le_bytes());
    archive.extend(container);
    archive
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn array_subcommands_convert_each_bank_then_run_and_apply_queued_motion() {
    let mut images = [fes::ImageState::default(), fes::ImageState::default()];
    for (image, x) in images.iter_mut().zip([10.5, 20.25]) {
        image.base.position = [x, -3.25];
        image.base.base_point = [1.25, 2.5];
        image.base.priority = 42;
        image.base.parent_priority = 22;
        image.base.color = 0x1234_5678;
    }
    let mut state = State::new(16);
    state.current = 77;
    let mut fes = fes::Interpreter::new(
        fes::Program::from_text(b"#DEFINE\nIMAGE sprite[2]\n#SUBCOMMAND\noneshot move motion px[0] : [0]\n#START\nsprite move (25+$0++)\nexit 0\n").unwrap(),
    ).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            fes.run(&mut state, 100).unwrap(),
            Step::Event(fes::Event::CreateObject(_) | fes::Event::Declarations(_))
        ));
        fes.resume();
    }
    let Step::Event(fes::Event::Subcommand(call)) = fes.run(&mut state, 100).unwrap() else {
        panic!()
    };
    assert_eq!(
        call.target,
        fes::ObjectReference {
            name: "sprite".into(),
            bank: -1
        }
    );
    assert_eq!(state.shared[0], 0);
    let resources = katze_system2::resource::ArchiveConfig::from_xml(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><INT><int00>
         <name>mot</name><int>motion.int</int><ext>kcs</ext><dir>loose</dir>
         </int00></INT></document>",
    )
    .unwrap();
    let target = resources.resolve(b"mot", &call.binding.target).unwrap();
    assert_eq!(target, b"motion.int/motion.kcs");
    let mut code = vec![];
    ins(&mut code, 1, &[(0x14, 0x8000_0018), (0x14, 0x8000_009c)]);
    ins(&mut code, 0x15, &[(4, 0x8000_0018), (0x44, 0)]);
    code.push(0x2d);
    ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    let bytes = archived_motion(&code);
    let archive = katze_system2::resource::KifArchive::parse(&bytes, 0).unwrap();
    let mut router = katze_system2::resource::ResourceRouter::default();
    router.slots[0].archive = Some(katze_system2::resource::ArchiveMount {
        path: "motion.int".encode_utf16().collect(),
        archive: &archive,
    });
    let loaded = router
        .read(
            &target,
            katze_system2::resource::ResourceMode::Archive,
            |_| panic!(),
        )
        .unwrap()
        .unwrap();
    let program = kcs::Program::parse(&loaded.bytes).unwrap();
    for (image, expected) in images.iter_mut().zip([26.0, 27.0]) {
        let arguments = fes
            .subcommand_arguments(
                &call.binding,
                &call.arguments,
                &image.base.subcommand_context(),
                &mut state,
            )
            .unwrap();
        let motion_state = kcs::MotionState::with_parameters(&arguments).unwrap();
        let mut queue = kcs::MotionQueue::default();
        queue.base_properties = image.base.motion_properties();
        let id = queue
            .insert(
                kcs::MotionTask::new(program.clone(), motion_state, 0).unwrap(),
                0,
            )
            .unwrap();
        assert_eq!(queue.group(id), Some(0));
        assert!(
            queue
                .update_object(0, &mut kcs::MotionHost::default(), 100)
                .unwrap()
        );
        assert!(queue.is_empty());
        image.base.apply_motion_properties(queue.properties());
        assert_eq!(image.base.position, [expected, -3.25]);
        assert_eq!(image.base.priority, 42);
        assert_eq!(image.base.parent_priority, 22);
        assert_eq!(image.base.color, 0x1234_5678);
    }
    assert_eq!((state.shared[0], state.current), (2, 77));
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

#[test]
fn subcommand_registration_freezes_targets_preserves_builtin_priority_and_evaluates_selectors_once()
{
    let mut state = State::new(16);
    state.strings.insert(1, b"first".to_vec());
    let mut vm = fes::Interpreter::new(fes::Program::from_text(b"#DEFINE\nIMAGE sprite[2]\n#SUBCOMMAND\noneshot move $str1 p[0] : [0]\noneshot load first p[0] : [0]\ncontinue MOVE first p[0] : [0]\n#START\nsprite[$0++] move 7\nsprite load 9\nsprite Move 8\nsprite MOVE 6\n#sprite.PUSH_L\n@ move 7\nsprite[@] MOVE 6\n").unwrap()).unwrap();
    assert!(vm.registered_subcommands().is_none());
    assert!(matches!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::CreateObject(_))
    ));
    vm.resume();
    assert!(matches!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Declarations(_))
    ));
    assert!(vm.registered_subcommands().unwrap().diagnostics.is_empty());
    state.strings.insert(1, b"second".to_vec());
    vm.resume();
    let event = vm.run(&mut state, 100).unwrap();
    let Step::Event(fes::Event::Subcommand(call)) = &event else {
        panic!()
    };
    assert_eq!(call.target.bank, 1);
    assert_eq!(call.binding.target, b"first");
    assert_eq!(vm.run(&mut state, 0).unwrap(), event);
    assert_eq!(state.shared[0], 1);
    vm.resume();
    assert!(matches!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Command(_))
    ));
    vm.resume();
    assert!(matches!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Command(_))
    ));
    vm.resume();
    let Step::Event(fes::Event::Subcommand(call)) = vm.run(&mut state, 100).unwrap() else {
        panic!()
    };
    assert_eq!(call.binding.mode, fes::SubcommandMode::Continue);
    assert_eq!(call.arguments, [b"6".to_vec()]);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    assert!(vm.dispatch_event("sprite", 1, "PUSH_L", None).unwrap());
    for _ in 0..2 {
        let Step::Event(fes::Event::Subcommand(call)) = vm.run(&mut state, 100).unwrap() else {
            panic!()
        };
        assert_eq!(
            call.target,
            fes::ObjectReference {
                name: "sprite".into(),
                bank: 1
            }
        );
        vm.resume();
    }
    assert_eq!(vm.run(&mut state, 100).unwrap(), Step::Yield);
    assert_eq!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Frame)
    );
}
