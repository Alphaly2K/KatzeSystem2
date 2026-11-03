use katze_system2::{
    Error, HandleAllocator, Step,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    runtime::fes::{ObjectState, Objects},
};
use std::collections::BTreeMap;

fn definition(kind: &'static str, name: &str, banks: u32) -> ObjectDefinition {
    ObjectDefinition {
        kind,
        name: name.into(),
        banks,
    }
}

fn object_command(
    objects: &mut Objects,
    interpreter: &mut fes::Interpreter,
    numbers: &mut State,
    bytes: &[u8],
) -> katze_system2::Result<bool> {
    objects.dispatch_object_command(
        interpreter,
        &fes::Line {
            offset: 100,
            bytes: bytes.to_vec(),
        },
        numbers,
    )
}

#[test]
fn plane_links_evaluate_once_stop_at_holes_or_controls_and_only_invalidate_changed_references() {
    for hole in [false, true] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        let ids = objects.define(&definition("CIMAGE", "item", 4)).unwrap();
        objects.get_mut(ids[1]).unwrap().state = ObjectState::Image(Box::default());
        objects.get_mut(ids[2]).unwrap().state = ObjectState::Scroll2(Box::default());
        objects.get_mut(ids[3]).unwrap().state = ObjectState::Button(Box::default());
        objects.set_parent(ids[3], Some(ids[0])).unwrap();
        objects.define(&definition("PLANE", "planes", 2)).unwrap();
        if hole {
            objects.remove(ids[2]).unwrap();
        }
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 9\n").unwrap()).unwrap();
        objects.publish_bindings(&mut interpreter);
        let mut numbers = State::new(4);
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item pl planes[($0+=1)] ($0+=100)"
            )
            .unwrap()
        );
        assert_eq!(numbers.shared[0], 1);
        let ObjectState::CImage(first) = &objects.get(ids[0]).unwrap().state else {
            panic!()
        };
        assert_eq!(
            first.plane,
            ObjectReference {
                name: "planes".into(),
                bank: 1
            }
        );
        assert!(first.updates.is_dirty());
        assert!(!first.updates.needs_rebuild());
        let ObjectState::Image(second) = &objects.get(ids[1]).unwrap().state else {
            panic!()
        };
        assert_eq!(second.base.plane, first.plane);
        let ObjectState::Button(child) = &objects.get(ids[3]).unwrap().state else {
            panic!()
        };
        assert!(child.image.base.plane.name.is_empty());
        assert!(child.image.base.updates.is_dirty());
        if !hole {
            let ObjectState::Scroll2(scroll) = &objects.get(ids[2]).unwrap().state else {
                panic!()
            };
            assert!(scroll.control.plane.name.is_empty());
            assert_eq!(scroll.control.updates, Default::default());
        }
        let ObjectState::CImage(first) = &mut objects.get_mut(ids[0]).unwrap().state else {
            panic!()
        };
        first.updates = Default::default();
        let ObjectState::Button(child) = &mut objects.get_mut(ids[3]).unwrap().state else {
            panic!()
        };
        child.image.base.updates = Default::default();
        numbers.shared[0] = 0;
        for command in [
            b"item[($0+=1)-1] pl planes[($0+=1)-1]".as_slice(),
            b"item pl",
            b"item pl [($0+=100)]",
        ] {
            assert!(object_command(&mut objects, &mut interpreter, &mut numbers, command).unwrap());
        }
        assert_eq!(numbers.shared[0], 2);
        let ObjectState::CImage(first) = &objects.get(ids[0]).unwrap().state else {
            panic!()
        };
        assert!(!first.updates.is_dirty());
        let ObjectState::Button(child) = &objects.get(ids[3]).unwrap().state else {
            panic!()
        };
        assert!(!child.image.base.updates.is_dirty());
    }
}

#[test]
fn object_commands_evaluate_once_and_keep_distinct_bank_and_type_traversal_rules() {
    for hole in [false, true] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        let handles = objects.define(&definition("CIMAGE", "item", 5)).unwrap();
        objects.get_mut(handles[1]).unwrap().state = ObjectState::Image(Box::default());
        objects.get_mut(handles[2]).unwrap().state = ObjectState::Base;
        objects.get_mut(handles[3]).unwrap().state = ObjectState::CPlane(Box::default());
        objects.set_parent(handles[3], Some(handles[0])).unwrap();
        objects
            .initialize(
                &ObjectInitialization {
                    target: ObjectReference {
                        name: "item".into(),
                        bank: -1,
                    },
                    attributes: ObjectAttributes {
                        display: Some(21),
                        priority: Some(7),
                        sequential_priority: true,
                        ..Default::default()
                    },
                },
                |name| Ok(name.to_vec()),
            )
            .unwrap();
        if hole {
            objects.remove(handles[1]).unwrap();
        }
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
        objects.publish_bindings(&mut interpreter);
        let mut numbers = State::new(4);
        let mut dispatch = |objects: &mut Objects, bytes: &[u8]| {
            object_command(objects, &mut interpreter, &mut numbers, bytes).unwrap()
        };
        assert!(dispatch(&mut objects, b"item size ($0+=1) ($0+=1)"));
        assert!(dispatch(&mut objects, b"item disp"));
        assert!(dispatch(&mut objects, b"item pri 8"));
        assert!(dispatch(&mut objects, b"item pos2 asset:5 ($0+=1)"));
        assert!(dispatch(&mut objects, b"item enable ($0+=1)"));
        assert!(!dispatch(&mut objects, b"item unknown ($0+=1)"));
        assert_eq!(numbers.shared[0], 4);
        for index in [0, 4] {
            let ObjectState::CImage(image) = &objects.get(handles[index]).unwrap().state else {
                panic!()
            };
            assert_eq!(image.size, if index == 0 { [1, 2] } else { [-1; 2] });
            assert_eq!(image.display, if index == 0 || !hole { 1 } else { 21 });
            assert_eq!(image.priority, if index == 0 || !hole { 28 } else { 31 });
            assert_eq!(image.parent_priority, 7 + index as i32);
            assert_eq!(image.position_resource.as_ref().unwrap().name, b"asset");
            assert_eq!(image.position_resource.as_ref().unwrap().frame, 5);
        }
        let ObjectState::CPlane(plane) = &objects.get(handles[3]).unwrap().state else {
            panic!()
        };
        assert_eq!(plane.size(), [800, 600]);
        assert_eq!(plane.display(), if hole { 21 } else { 1 });
        assert_eq!(plane.priority(), if hole { 10 } else { 8 });
        assert!(plane.position_resource().is_none());

        let ObjectState::CImage(image) = &mut objects.get_mut(handles[0]).unwrap().state else {
            panic!()
        };
        image.updates = Default::default();
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item[0] pri 28"
            )
            .unwrap()
        );
        assert!(
            object_command(&mut objects, &mut interpreter, &mut numbers, b"item[0] pri").unwrap()
        );
        let ObjectState::CImage(image) = &objects.get(handles[0]).unwrap().state else {
            panic!()
        };
        assert!(!image.updates.priority_changed());
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item[0] pri 8"
            )
            .unwrap()
        );
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item[0] size"
            )
            .unwrap()
        );
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item[0] disp 0"
            )
            .unwrap()
        );
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"item[0] pos2"
            )
            .unwrap()
        );
        let ObjectState::CImage(image) = &objects.get(handles[0]).unwrap().state else {
            panic!()
        };
        assert_eq!(image.size, [0; 2]);
        assert_eq!(image.display, 0);
        assert!(image.updates.priority_changed());
        assert!(image.updates.needs_rebuild());
        assert_eq!(image.position_resource.as_ref().unwrap().frame, 5);
        let before = numbers.shared.clone();
        assert!(
            object_command(
                &mut objects,
                &mut interpreter,
                &mut numbers,
                b"missing size ($0+=1) ($0+=1)"
            )
            .is_err()
        );
        assert_eq!(numbers.shared, before);
    }
}

#[test]
fn keypos_evaluates_once_skips_other_types_and_stops_at_missing_banks() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects.define(&definition("BUTTON", "item", 6)).unwrap();
    objects.get_mut(handles[0]).unwrap().state = ObjectState::CImage(Box::default());
    objects.get_mut(handles[1]).unwrap().state = ObjectState::Base;
    objects.get_mut(handles[2]).unwrap().state = ObjectState::Image(Box::default());
    objects.remove(handles[4]).unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(4);
    for line in [
        b"item keypos ($0+=1) ($0+=1) ($0+=1)".as_slice(),
        b"item[3] size 32 16",
    ] {
        assert!(object_command(&mut objects, &mut interpreter, &mut numbers, line).unwrap());
    }
    assert_eq!(numbers.shared[0], 3);
    let ObjectState::Button(last) = &objects.get(handles[5]).unwrap().state else {
        panic!()
    };
    assert_eq!(last.image.base.key_position, [0; 2]);
    for index in [0, 2, 3] {
        let state = &objects.get(handles[index]).unwrap().state;
        let image = match state {
            ObjectState::CImage(image) => image.as_ref(),
            ObjectState::Image(image) => &image.base,
            ObjectState::Button(button) => &button.image.base,
            _ => panic!(),
        };
        assert_eq!(image.key_position_mode, 1);
        assert_eq!(image.key_position, [2, 3]);
        assert!(!image.updates.is_dirty());
    }
    assert!(
        object_command(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            b"item[3] keypos 0 ($0+=1) ($0+=1)"
        )
        .unwrap()
    );
    assert_eq!(numbers.shared[0], 5);
    let ObjectState::Button(button) = &objects.get(handles[3]).unwrap().state else {
        panic!()
    };
    assert_eq!(button.image.base.key_position, [26, 11]);
    assert!(
        object_command(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            b"item[3] keypos"
        )
        .unwrap()
    );
    let ObjectState::Button(button) = &objects.get(handles[3]).unwrap().state else {
        panic!()
    };
    assert_eq!(button.image.base.key_position, [50, 19]);
}

#[test]
fn id_commands_query_logical_frames_and_guard_arrays_and_shared_destinations() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let single = objects.define(&definition("IMAGE", "single", 1)).unwrap()[0];
    let handles = objects.define(&definition("IMAGE", "item", 2)).unwrap();
    let base = objects.define(&definition("BASE", "base", 1)).unwrap()[0];
    let ObjectState::Image(image) = &mut objects.get_mut(single).unwrap().state else {
        panic!()
    };
    image.image_number = 22;
    let ObjectState::Image(image) = &mut objects.get_mut(handles[0]).unwrap().state else {
        panic!()
    };
    image.frames[2] = 11;
    image.frame_table_enabled = true;
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(4);
    for line in [b"item setid 2".as_slice(), b"item getid ($0+=1)"] {
        assert!(object_command(&mut objects, &mut interpreter, &mut numbers, line).unwrap());
    }
    assert_eq!(numbers.shared, [0; 4]);
    for (line, expected) in [
        (b"single getid ($0+=1)".as_slice(), [1, 22, 0, 0]),
        (b"item[0] getid 2", [1, 22, 2, 0]),
        (b"single getid -1", [1, 22, 2, 0]),
        (b"single getid 99", [1, 22, 2, 0]),
        (b"single getid", [22, 22, 2, 0]),
        (b"single setid", [22, 22, 2, 0]),
        (b"single getid", [0, 22, 2, 0]),
        (b"item[0] setid 32", [0, 22, 2, 0]),
        (b"item[0] getid 3", [0, 22, 2, 2]),
    ] {
        assert!(object_command(&mut objects, &mut interpreter, &mut numbers, line).unwrap());
        assert_eq!(numbers.shared, expected);
    }
    let ObjectState::Image(image) = &objects.get(handles[0]).unwrap().state else {
        panic!()
    };
    assert_eq!(image.selected_frame(), 11);
    assert!(image.base.updates.needs_rebuild());
    assert!(
        object_command(
            &mut objects,
            &mut interpreter,
            &mut numbers,
            b"base getid 0"
        )
        .is_err()
    );
    assert_eq!(objects.get(base).unwrap().state, ObjectState::Base);
    assert_eq!(numbers.shared, [0, 22, 2, 2]);

    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 1\n#single.FOCUS\n").unwrap())
            .unwrap();
    objects.publish_bindings(&mut interpreter);
    numbers.current = 13;
    assert!(
        interpreter
            .dispatch_event("single", -1, "FOCUS", None)
            .unwrap()
    );
    for line in [b"@ setid (@+2)".as_slice(), b"@ getid 3"] {
        assert!(object_command(&mut objects, &mut interpreter, &mut numbers, line).unwrap());
    }
    assert_eq!(numbers.shared[3], 1);
    assert_eq!(numbers.current, 13);
}

#[test]
fn animation_commands_expand_banks_skip_other_types_and_preserve_the_play_switch() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects.define(&definition("IMAGE", "item", 3)).unwrap();
    objects.get_mut(handles[1]).unwrap().state = ObjectState::CImage(Box::default());
    objects.remove(handles[0]).unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let mut numbers = State::new(0);
    let mut dispatch = |objects: &mut Objects, bytes: &[u8]| {
        objects.dispatch_image_animation(
            &mut interpreter,
            &fes::Line {
                offset: 100,
                bytes: bytes.to_vec(),
            },
            &mut numbers,
        )
    };
    assert!(dispatch(&mut objects, b"item setanm").unwrap());
    assert!(
        objects
            .get(handles[1])
            .unwrap()
            .image_animation_request()
            .is_none()
    );
    let request = objects
        .get(handles[2])
        .unwrap()
        .image_animation_request()
        .unwrap();
    assert_eq!(request.name(), b".anm");
    assert_eq!(request.playing(), 0);
    assert!(request.is_pending());
    for (line, playing, name) in [
        (b"item anm 0".as_slice(), 1, b".anm".as_slice()),
        (b"item[2] setanm dir.with/file", 1, b"dir.with/file.anm"),
        (b"item[2] anm sToP", 0, b"dir.with/file.anm"),
        (b"item[2] setanm explicit.custom", 0, b"explicit.custom"),
        (b"item[2] setanm tail.", 0, b"tail.anm"),
    ] {
        assert!(dispatch(&mut objects, line).unwrap());
        let request = objects
            .get(handles[2])
            .unwrap()
            .image_animation_request()
            .unwrap();
        assert_eq!((request.playing(), request.name()), (playing, name));
    }
    assert!(dispatch(&mut objects, b"missing anm").unwrap());
    assert!(!dispatch(&mut objects, b"item play").unwrap());
    assert!(
        dispatch(
            &mut objects,
            &[b"item[2] setanm ".as_slice(), &[b'x'; 256]].concat()
        )
        .is_err()
    );
    assert_eq!(
        objects
            .get(handles[2])
            .unwrap()
            .image_animation_request()
            .unwrap()
            .name(),
        b"tail.anm"
    );
}

#[test]
fn definitions_keep_first_bindings_and_count_while_owning_duplicate_objects() {
    let handles = HandleAllocator::default();
    let foreign = handles.allocate().unwrap();
    let mut objects = Objects::new(handles.clone()).unwrap();
    assert_ne!(objects.top(), foreign);
    assert_eq!(objects.lookup("_TOPOBJ_", 0), Some(objects.top()));
    assert_eq!(objects.count("_TOPOBJ_"), 1);
    assert_eq!(objects.get(objects.top()).unwrap().parent(), None);

    let first = objects.define(&definition("CIMAGE", "item", 3)).unwrap();
    let duplicate = objects.define(&definition("IMAGE", "item", 2)).unwrap();
    let extension = objects.define(&definition("BASE", "item", 5)).unwrap();
    let upper = objects.define(&definition("IMAGE", "Item", 1)).unwrap()[0];
    let top_duplicate = objects.define(&definition("BASE", "_TOPOBJ_", 1)).unwrap()[0];
    assert_eq!(objects.count("item"), 3);
    assert_eq!(objects.lookup("item", -1), None);
    assert_eq!(objects.lookup("item", 5), None);
    assert_eq!(objects.lookup("Item", 0), Some(upper));
    assert_eq!(objects.lookup("_TOPOBJ_", 0), Some(objects.top()));
    assert_ne!(top_duplicate, objects.top());
    for bank in 0..5 {
        let handle = if bank < 3 {
            first[bank]
        } else {
            extension[bank]
        };
        assert_eq!(objects.lookup("item", bank as i32), Some(handle));
        assert_eq!(objects.get(handle).unwrap().parent(), Some(objects.top()));
    }
    for handle in duplicate {
        assert_eq!(objects.get(handle).unwrap().state.kind(), "IMAGE");
        assert_eq!(objects.get(handle).unwrap().parent(), Some(objects.top()));
    }
    let order = objects.update_order().collect::<Vec<_>>();
    assert_eq!(&order[..3], first.as_slice());
    assert_eq!(&order[3..5], &first[..2]);
    assert_eq!(
        &order[5..10],
        &[first[0], first[1], first[2], extension[3], extension[4]]
    );
    assert_eq!(&order[10..], &[upper, objects.top()]);
    assert_eq!(objects.len(), 13);
    assert_eq!(objects.get(objects.top()).unwrap().children().count(), 12);
    assert!(objects.get(foreign).is_none());
    let next = handles.allocate().unwrap();
    for invalid in [
        definition("UNKNOWN", "unsupported", 1),
        definition("IMAGE", "", 1),
        definition("IMAGE", &"a".repeat(32), 1),
        definition("IMAGE", "zero", 0),
        definition("IMAGE", "large", 1_000_000),
    ] {
        assert!(objects.define(&invalid).is_err());
    }
    assert_eq!(objects.len(), 13);
    assert_eq!(handles.allocate().unwrap(), next + 1);
}

#[test]
fn interpreter_events_initialize_actual_bank_types_and_publish_object_identities() {
    let source = b"#DEFINE\nBASE anchor\nCIMAGE item[2]\nIMAGE item[4]\n#OBJECT PARENT POS SIZE COLOR PRI DISP\nitem anchor 3,5 6,8 0x12345678 7 1\nitem[3] anchor -2,4 1,2 7 11 1\n#OBJECT FILE IDN ID.1\nitem[2] texture 1 19\n#START\n$0=item[3]\nexit 0\n";
    for budget in [1, 100] {
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        interpreter
            .named
            .insert("_FRAME_".into(), BTreeMap::from([(0, 9)]));
        let mut numbers = State::new(8);
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        objects.publish_bindings(&mut interpreter);
        let mut transforms = Vec::new();
        let mut events = 0;
        loop {
            match interpreter.run(&mut numbers, budget).unwrap() {
                Step::Yield => continue,
                Step::Event(event) => {
                    events += 1;
                    match event {
                        fes::Event::CreateObject(definition) => {
                            objects.define(&definition).unwrap();
                            objects.publish_bindings(&mut interpreter);
                        }
                        fes::Event::InitializeObject(initialization) => objects
                            .initialize(&initialization, |name| {
                                transforms.push(name.to_vec());
                                Ok([b"art/".as_slice(), name].concat())
                            })
                            .unwrap(),
                        fes::Event::Exit(0) => break,
                        event => panic!("unexpected {event:?}"),
                    }
                    interpreter.resume();
                }
                step => panic!("unexpected {step:?}"),
            }
        }
        assert_eq!(events, 7);
        assert_eq!(objects.len(), 8);
        let anchor = objects.lookup("anchor", 0).unwrap();
        for bank in 0..2 {
            let object = objects.get(objects.lookup("item", bank).unwrap()).unwrap();
            assert_eq!(object.parent(), Some(anchor));
            let ObjectState::CImage(state) = &object.state else {
                panic!()
            };
            assert_eq!(state.position, [3., 5.]);
            assert_eq!(state.size, [6, 8]);
            assert_eq!(state.color, 0x12345678);
            assert_eq!(state.priority, 27 + bank);
            assert_eq!(state.parent_priority, 7 + bank);
            assert_eq!(state.display, 1);
        }
        let ObjectState::Image(state) = &objects
            .get(objects.lookup("item", 2).unwrap())
            .unwrap()
            .state
        else {
            panic!()
        };
        assert_eq!(state.base.position, [0.; 2]);
        assert_eq!(state.file, b"art/texture");
        assert_eq!(state.selected_frame(), 19);
        assert_eq!(transforms, [b"texture"]);
        let handle = objects.lookup("item", 3).unwrap();
        let ObjectState::Image(state) = &objects.get(handle).unwrap().state else {
            panic!()
        };
        assert_eq!(state.base.position, [-2., 4.]);
        assert_eq!(state.base.size, [-1; 2]);
        assert_eq!(state.base.color, -1);
        assert_eq!(state.base.priority, 31);
        assert_eq!(numbers.shared[0], handle as i32);
        assert_eq!(interpreter.named["_FRAME_"][&0], 9);
    }
}

#[test]
fn image_property_changes_invalidate_descendants_without_rebuilding_their_sources() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let parent = objects.define(&definition("CIMAGE", "group", 1)).unwrap()[0];
    let child = objects.define(&definition("IMAGE", "child", 1)).unwrap()[0];
    let plane = objects.define(&definition("CPLANE", "panel", 1)).unwrap()[0];
    objects.set_parent(child, Some(parent)).unwrap();
    objects.set_parent(plane, Some(child)).unwrap();
    objects
        .initialize(
            &ObjectInitialization {
                target: ObjectReference {
                    name: "group".into(),
                    bank: 0,
                },
                attributes: ObjectAttributes {
                    position: [Some(3), Some(5), None],
                    ..Default::default()
                },
            },
            |_| unreachable!(),
        )
        .unwrap();
    let ObjectState::Image(state) = &objects.get(child).unwrap().state else {
        panic!()
    };
    assert!(state.base.updates.is_dirty());
    assert!(!state.base.updates.needs_rebuild());
    let ObjectState::CPlane(state) = &objects.get(plane).unwrap().state else {
        panic!()
    };
    assert!(state.is_dirty());
    assert!(!state.needs_rebuild());
}

#[test]
fn reparenting_and_removal_keep_bank_holes_and_failed_initialization_changes() {
    let handles = HandleAllocator::default();
    let mut objects = Objects::new(handles).unwrap();
    let banks = objects.define(&definition("IMAGE", "item", 3)).unwrap();
    let hidden = objects.define(&definition("IMAGE", "item", 3)).unwrap();
    objects.set_parent(banks[1], Some(banks[0])).unwrap();
    objects.set_parent(banks[2], Some(banks[1])).unwrap();
    assert!(objects.set_parent(banks[0], Some(banks[2])).is_err());
    assert!(objects.set_parent(banks[0], Some(u32::MAX)).is_err());
    assert!(objects.set_parent(objects.top(), None).is_err());
    assert_eq!(objects.get(banks[0]).unwrap().parent(), Some(objects.top()));
    objects.set_parent(banks[2], None).unwrap();
    let initialization = ObjectInitialization {
        target: ObjectReference {
            name: "item".into(),
            bank: 2,
        },
        attributes: ObjectAttributes {
            parent: Some(ObjectReference {
                name: "item".into(),
                bank: 1,
            }),
            file: Some(b"broken".to_vec()),
            position: [Some(9), None, None],
            ..Default::default()
        },
    };
    assert!(
        objects
            .initialize(&initialization, |_| Err(Error::new(7, "conversion failed")))
            .is_err()
    );
    assert_eq!(objects.get(banks[2]).unwrap().parent(), Some(banks[1]));
    let ObjectState::Image(state) = &objects.get(banks[2]).unwrap().state else {
        panic!()
    };
    assert_eq!(state.requested_file, b"broken");
    assert!(state.file.is_empty());
    assert_eq!(state.base.position, [0.; 2]);

    let removed = objects.remove(banks[0]).unwrap();
    assert_eq!(
        removed
            .iter()
            .map(|object| object.handle())
            .collect::<Vec<_>>(),
        [banks[2], banks[1], banks[0]]
    );
    assert_eq!(objects.count("item"), 3);
    for handle in hidden {
        assert!(objects.get(handle).is_some());
    }
    for bank in 0..3 {
        assert_eq!(objects.lookup("item", bank), None);
    }
    let new = objects.define(&definition("BASE", "item", 2)).unwrap();
    assert_eq!(objects.count("item"), 3);
    assert_eq!(objects.lookup("item", 0), Some(new[0]));
    assert_eq!(objects.lookup("item", 1), Some(new[1]));
    assert_eq!(objects.lookup("item", 2), None);
    assert!(new[0] > *banks.last().unwrap());
    let all = ObjectInitialization {
        target: ObjectReference {
            name: "item".into(),
            bank: -1,
        },
        attributes: ObjectAttributes {
            parent: Some(ObjectReference {
                name: "missing".into(),
                bank: 0,
            }),
            ..Default::default()
        },
    };
    assert!(objects.initialize(&all, |_| unreachable!()).is_err());
    assert_eq!(objects.get(new[0]).unwrap().parent(), None);
    assert_eq!(objects.get(new[1]).unwrap().parent(), None);
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit 0\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    assert_eq!(
        interpreter.named["item"],
        BTreeMap::from([(0, new[0] as i32), (1, new[1] as i32)])
    );
    assert_eq!(
        interpreter
            .object_reference(b"item", &mut State::new(8))
            .unwrap()
            .bank,
        -1
    );
    let invalid = ObjectInitialization {
        target: ObjectReference {
            name: "item".into(),
            bank: i32::MAX,
        },
        attributes: ObjectAttributes::default(),
    };
    assert!(objects.initialize(&invalid, |_| unreachable!()).is_err());
    assert!(objects.remove(objects.top()).is_err());
    assert!(objects.remove(banks[0]).is_err());
}
