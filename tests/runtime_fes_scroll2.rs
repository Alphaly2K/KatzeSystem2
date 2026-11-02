use katze_system2::{
    HandleAllocator, Step,
    expression::State,
    fes::{self, ObjectReference},
    runtime::fes::{ObjectState, Objects},
};

fn control(objects: &Objects, bank: i32) -> &fes::Scroll2State {
    let handle = objects.lookup("bar", bank).unwrap();
    let ObjectState::Scroll2(state) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    state
}

fn command(objects: &mut Objects, vm: &mut fes::Interpreter, numbers: &mut State, bytes: &[u8]) {
    assert!(
        objects
            .dispatch_object_command(
                vm,
                &fes::Line {
                    offset: 0,
                    bytes: bytes.to_vec()
                },
                numbers
            )
            .unwrap()
    );
}

#[test]
fn scroll2_scripts_keep_page_length_queries_and_load_traversal_independent() {
    let source = b"#DEFINE\nSCROLL2 bar[3]\n#OBJECT FILE PL POS2 PRI DISP ENABLE\nbar original panel[1] bounds:3 7 1 9\n#START\nbar page ($0+=1) ($0+=1) ($0+=1) ($0+=100)\nbar[1] pos -7 2 3\nbar getpos ($3+=1) ($3+=1) ($3+=1)\nbar[1] getpos 4 5 6\nbar load replacement ($1+=1) ($2+=100)\nbar[1] settbl 1 7\nbar pos2 ignored:9 11\nwait 99\n";
    for budget in [1, 4096] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        let mut numbers = State::new(8);
        loop {
            match vm.run(&mut numbers, budget).unwrap() {
                Step::Yield => (),
                Step::Event(fes::Event::CreateObject(definition)) => {
                    objects.define(&definition).unwrap();
                    objects.publish_bindings(&mut vm);
                    vm.resume();
                }
                Step::Event(fes::Event::InitializeObject(initialization)) => {
                    objects.initialize(&initialization, |_| panic!()).unwrap();
                    vm.resume();
                }
                Step::Event(fes::Event::Command(line)) => {
                    command(&mut objects, &mut vm, &mut numbers, &line.bytes);
                    vm.resume();
                }
                Step::Event(fes::Event::Wait(99)) => break,
                step => panic!("{step:?}"),
            }
        }
        assert_eq!(&numbers.shared[..7], &[3, 3, 0, 0, -7, 0, 0]);
        for bank in 0..3 {
            let state = control(&objects, bank);
            assert_eq!((state.control.range, state.page), ([1, 2], 3));
            assert_eq!(state.control.position, if bank == 1 { -7 } else { 0 });
            assert_eq!(state.control.priority, 27);
            assert_eq!((state.control.display, state.control.enabled), (1, 9));
            assert_eq!(state.control.file, b"replacement");
            assert_eq!(
                state.control.plane,
                ObjectReference {
                    name: "panel".into(),
                    bank: 1
                }
            );
            assert_eq!(state.control.rectangle, [0; 4]);
            let metadata = state.control.position_resource.as_ref().unwrap();
            assert_eq!(
                (&metadata.name[..], metadata.frame),
                (b"bounds".as_slice(), 3)
            );
            assert_eq!(state.control.updates.frames, bank == 1);
        }
        let removed = objects.lookup("bar", 1).unwrap();
        objects.remove(removed).unwrap();
        command(&mut objects, &mut vm, &mut numbers, b"bar page -3 7 2");
        command(&mut objects, &mut vm, &mut numbers, b"bar load new ($1+=1)");
        assert_eq!(numbers.shared[1], 4);
        for bank in [0, 2] {
            let state = control(&objects, bank);
            assert_eq!((state.control.range, state.page), ([-3, 7], 2));
            assert_eq!(
                state.control.file,
                if bank == 0 {
                    b"new".as_slice()
                } else {
                    b"replacement".as_slice()
                }
            );
        }
    }
}

#[test]
fn linked_scroll2_navigation_subtracts_pages_with_wrapping_and_consumes_range_requests() {
    use katze_system2::runtime::fes::NavigationOutcome;
    for (range, page, position, direction, expected) in [
        ([0, 9], 3, 5, 3, Some(6)),
        ([0, 9], 3, 6, 3, None),
        ([0, 3], -3, 3, 3, Some(4)),
        ([0, i32::MIN], 1, i32::MAX - 1, 3, Some(i32::MAX)),
        ([i32::MIN, i32::MAX], -1, i32::MIN + 1, 2, Some(i32::MIN)),
        ([0, 9], 3, 5, 0, None),
    ] {
        let mut objects = Objects::new(HandleAllocator::default()).unwrap();
        for (kind, name) in [("SCROLL2", "bar"), ("BUTTON", "nav")] {
            objects
                .define(&fes::ObjectDefinition {
                    kind,
                    name: name.into(),
                    banks: 1,
                })
                .unwrap();
        }
        let nav = ObjectReference {
            name: "nav".into(),
            bank: 0,
        };
        let handle = objects.lookup(&nav.name, nav.bank).unwrap();
        let ObjectState::Button(button) = &mut objects.get_mut(handle).unwrap().state else {
            panic!()
        };
        button.keyblock = [0; 3];
        button
            .set_control(Some(&ObjectReference {
                name: "bar".into(),
                bank: 0,
            }))
            .unwrap();
        let handle = objects.lookup("bar", 0).unwrap();
        let ObjectState::Scroll2(scroll) = &mut objects.get_mut(handle).unwrap().state else {
            panic!()
        };
        scroll.set_page(range, page);
        scroll.control.position = position;
        scroll.control.enabled = 0;
        let blocks = [fes::KeyBlock {
            size: [1, 1],
            neighbors: [-1; 4],
        }];
        assert_eq!(
            objects
                .navigate_button_with_controls(&blocks, &nav, direction)
                .unwrap(),
            if expected.is_some() {
                NavigationOutcome::Handled
            } else {
                NavigationOutcome::Unhandled
            }
        );
        let scroll = control(&objects, 0);
        assert_eq!(scroll.control.position, expected.unwrap_or(position));
        assert!(!scroll.control.updates.range);
        assert_eq!(scroll.control.updates.position, expected.is_some());
        assert_eq!(scroll.control.notification, i32::from(expected.is_some()));
    }
}
