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
    assert_eq!(objects.len(), 13);
    assert_eq!(objects.get(objects.top()).unwrap().children().count(), 12);
    assert!(objects.get(foreign).is_none());
    let next = handles.allocate().unwrap();
    for invalid in [
        definition("BUTTON", "unsupported", 1),
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
