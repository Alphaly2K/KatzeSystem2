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
