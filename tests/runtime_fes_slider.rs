use katze_system2::{
    HandleAllocator,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    kcs::{MotionHost, Program},
    runtime::fes::{ObjectState, Objects},
};

fn reference(name: &str, bank: i32) -> ObjectReference {
    ObjectReference {
        name: name.into(),
        bank,
    }
}

fn slider(objects: &Objects, handle: u32) -> &fes::SliderState {
    match &objects.get(handle).unwrap().state {
        ObjectState::Slider(slider) => slider,
        ObjectState::Scroll2(scroll) => &scroll.control,
        _ => panic!(),
    }
}

#[test]
fn slider_commands_keep_query_and_load_evaluation_distinct_across_bank_holes() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects
        .define(&ObjectDefinition {
            kind: "SLIDER",
            name: "item".into(),
            banks: 3,
        })
        .unwrap();
    let button = objects
        .define(&ObjectDefinition {
            kind: "BUTTON",
            name: "button".into(),
            banks: 1,
        })
        .unwrap()[0];
    objects.set_parent(handles[2], Some(handles[0])).unwrap();
    objects
        .initialize(
            &ObjectInitialization {
                target: reference("item", -1),
                attributes: ObjectAttributes {
                    priority: Some(5),
                    sequential_priority: true,
                    position_resource: Some(b"bounds:3".to_vec()),
                    ..Default::default()
                },
            },
            |_| panic!(),
        )
        .unwrap();
    assert!(
        handles
            .iter()
            .all(|&id| slider(&objects, id).priority == 25)
    );
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(8);
    let mut command = |objects: &mut Objects, bytes: &[u8]| {
        assert!(
            objects
                .dispatch_object_command(
                    &mut interpreter,
                    &fes::Line {
                        offset: 0,
                        bytes: bytes.to_vec()
                    },
                    &mut numbers
                )
                .unwrap()
        );
    };
    command(&mut objects, b"item page ($0+=1) ($0+=1) ($0+=1)");
    command(&mut objects, b"item pos ($0+=1) ($0+=1) ($0+=1)");
    command(&mut objects, b"item[0] pos2 ignored:9 7");
    command(&mut objects, b"item[0] enable 0");
    assert!(slider(&objects, handles[0]).updates.dirty);
    assert!(!slider(&objects, handles[2]).updates.dirty);
    command(&mut objects, b"item[0] disp 0");
    assert!(slider(&objects, handles[2]).updates.dirty);
    command(&mut objects, b"item pri -20");
    command(&mut objects, b"button attach item[0]");
    for &handle in &handles {
        let state = slider(&objects, handle);
        assert_eq!(
            (state.range, state.position, state.notification),
            ([1, 2], 4, 0)
        );
        assert_eq!(state.priority, 0);
        assert_eq!(state.position_resource.as_ref().unwrap().name, b"bounds");
        assert_eq!(state.position_resource.as_ref().unwrap().frame, 3);
    }
    let ObjectState::Button(state) = &objects.get(button).unwrap().state else {
        panic!()
    };
    assert_eq!(state.control, Some(reference("item", 0)));
    objects.remove(handles[1]).unwrap();
    command(&mut objects, b"item page -3 7 ($0+=1)");
    command(&mut objects, b"item load control ($0+=1) ($0+=1)");
    command(&mut objects, b"item getpos ($0+=1) ($0+=1) ($0+=1)");
    command(&mut objects, b"item[2] getpos 1 2 3");
    command(&mut objects, b"button attach missing[99]");
    assert_eq!(numbers.shared[..4], [8, 4, 0, 0]);
    assert_eq!(slider(&objects, handles[0]).file, b"control");
    assert!(slider(&objects, handles[0]).updates.file);
    assert!(slider(&objects, handles[2]).file.is_empty());
    assert_eq!(slider(&objects, handles[2]).range, [-3, 7]);
    let ObjectState::Button(state) = &objects.get(button).unwrap().state else {
        panic!()
    };
    assert_eq!(state.control, Some(reference("missing", 99)));
}

#[test]
fn sliding_controls_motion_uses_alpha_without_invalidating_descendants_or_replacing_numeric_state()
{
    let mut code = Vec::new();
    for (address, value) in [(0x8000_0018u32, 0xdeadbeefu32), (0x8000_0070, 0x81345678)] {
        code.push(0x15);
        for word in [address, value] {
            code.push(3);
            code.extend(word.to_le_bytes());
        }
        code.push(0x2d);
    }
    code.push(0x3b);
    let program = Program::from_code(&code, vec![], 32).unwrap();
    for kind in ["SLIDER", "SCROLL2"] {
        for budget in [1, 4096] {
            let mut objects = Objects::new(HandleAllocator::default()).unwrap();
            let parent = objects
                .define(&ObjectDefinition {
                    kind,
                    name: "slider".into(),
                    banks: 1,
                })
                .unwrap()[0];
            let child = objects
                .define(&ObjectDefinition {
                    kind,
                    name: "child".into(),
                    banks: 1,
                })
                .unwrap()[0];
            objects.set_parent(child, Some(parent)).unwrap();
            let state = match &mut objects.get_mut(parent).unwrap().state {
                ObjectState::Slider(state) => state,
                ObjectState::Scroll2(state) => &mut state.control,
                _ => panic!(),
            };
            state.alpha = 0x1234;
            state.range = [-4, 8];
            state.position = 99;
            objects
                .start_motion(parent, program.clone(), &[], 0)
                .unwrap();
            let mut host = MotionHost::default();
            while !objects.update_motions(7, &mut host, budget).unwrap() {}
            let state = slider(&objects, parent);
            assert_eq!(
                (state.alpha, state.position, state.range),
                (0x81, 99, [-4, 8])
            );
            assert!(state.updates.alpha);
            assert!(!state.updates.dirty);
            assert!(!slider(&objects, child).updates.dirty);
            let properties = objects.get(parent).unwrap().motions().base_properties;
            assert_eq!(properties[28], 0x34000000);
            assert_eq!(properties[6], 0);
        }
    }
}

#[test]
fn frame_table_commands_keep_evaluation_bank_holes_and_media_flags_distinct() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects
        .define(&ObjectDefinition {
            kind: "SLIDER",
            name: "item".into(),
            banks: 4,
        })
        .unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 99\n").unwrap()).unwrap();
    let mut numbers = State::new(8);
    let dispatch = |objects: &mut Objects,
                    interpreter: &mut fes::Interpreter,
                    numbers: &mut State,
                    bytes: &[u8]| {
        assert!(
            objects
                .dispatch_object_command(
                    interpreter,
                    &fes::Line {
                        offset: 0,
                        bytes: bytes.to_vec()
                    },
                    numbers
                )
                .unwrap()
        );
    };
    objects.publish_bindings(&mut interpreter);
    dispatch(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item settbl ($0+=1) ($0+=1) ($0+=100)",
    );
    assert_eq!(numbers.shared[0], 2);
    for &handle in &handles {
        assert_eq!(slider(&objects, handle).frames, [0, 2, 2, 3]);
        let ObjectState::Slider(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        assert!(state.updates.frames);
        state.updates.frames = false;
    }
    objects.remove(handles[2]).unwrap();
    dispatch(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item settbl 3 -1",
    );
    for (index, &handle) in handles.iter().enumerate().filter(|(index, _)| *index != 2) {
        assert_eq!(
            slider(&objects, handle).frames[3],
            if index < 2 { u32::MAX } else { 3 }
        );
        assert_eq!(slider(&objects, handle).updates.frames, index < 2);
    }
    objects.get_mut(handles[1]).unwrap().state = ObjectState::Base;
    objects
        .define(&ObjectDefinition {
            kind: "BASE",
            name: "item".into(),
            banks: 3,
        })
        .unwrap();
    objects.publish_bindings(&mut interpreter);
    dispatch(
        &mut objects,
        &mut interpreter,
        &mut numbers,
        b"item settbl 0 9",
    );
    assert_eq!(slider(&objects, handles[0]).frames[0], 9);
    assert_eq!(slider(&objects, handles[3]).frames[0], 9);

    fn image(objects: &mut Objects, handle: u32) -> &mut fes::ImageState {
        match &mut objects.get_mut(handle).unwrap().state {
            ObjectState::Image(image) => image,
            ObjectState::Button(button) => &mut button.image,
            _ => unreachable!(),
        }
    }
    for (index, kind) in ["IMAGE", "BUTTON"].into_iter().enumerate() {
        let name = kind.to_ascii_lowercase();
        let handle = objects
            .define(&ObjectDefinition {
                kind,
                name: name.clone(),
                banks: 1,
            })
            .unwrap()[0];
        objects.publish_bindings(&mut interpreter);
        let state = image(&mut objects, handle);
        state.frame_mapping_changed = false;
        state.frame_table_enabled = false;
        let updates = state.base.updates.clone();
        dispatch(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            format!("{name} settbl -1 ($0+=1)").as_bytes(),
        );
        let state = image(&mut objects, handle);
        assert!(!state.frame_mapping_changed && !state.frame_table_enabled);
        assert_eq!(numbers.shared[0], 3 + index as i32);
        dispatch(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            format!("{name} settbl 2 7").as_bytes(),
        );
        let state = image(&mut objects, handle);
        assert_eq!(state.frames[2], 7);
        assert!(state.frame_mapping_changed && state.frame_table_enabled);
        assert_eq!(state.base.updates, updates);
    }
}
