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
    let ObjectState::Slider(slider) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    slider
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
fn slider_motion_uses_alpha_without_invalidating_descendants_or_replacing_numeric_state() {
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
    for budget in [1, 4096] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        let parent = objects
            .define(&ObjectDefinition {
                kind: "SLIDER",
                name: "slider".into(),
                banks: 1,
            })
            .unwrap()[0];
        let child = objects
            .define(&ObjectDefinition {
                kind: "SLIDER",
                name: "child".into(),
                banks: 1,
            })
            .unwrap()[0];
        objects.set_parent(child, Some(parent)).unwrap();
        let ObjectState::Slider(state) = &mut objects.get_mut(parent).unwrap().state else {
            panic!()
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
