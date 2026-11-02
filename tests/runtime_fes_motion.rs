use katze_system2::{
    HandleAllocator,
    fes::ObjectDefinition,
    kcs::{MotionHost, Program},
    runtime::fes::{ObjectState, Objects},
};

fn ins(code: &mut Vec<u8>, opcode: u8, operands: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in operands {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn movement(stop: bool) -> Program {
    let mut code = Vec::new();
    ins(&mut code, 1, &[(0x14, 0x8000_0018), (0x14, 0x8000_009c)]);
    ins(&mut code, 0x15, &[(4, 0x8000_0018), (0x44, 0)]);
    code.push(0x2d);
    if stop {
        ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    }
    code.push(0x3b);
    Program::from_code(&code, Vec::new(), 64).unwrap()
}

fn setup() -> (Objects, Vec<u32>) {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let handles = objects
        .define(&ObjectDefinition {
            kind: "CIMAGE",
            name: "item".into(),
            banks: 2,
        })
        .unwrap();
    objects.set_parent(handles[1], Some(handles[0])).unwrap();
    for (&handle, x) in handles.iter().zip([10., 20.]) {
        let ObjectState::CImage(image) = &mut objects.get_mut(handle).unwrap().state else {
            panic!()
        };
        image.position = [x, 0.];
        image.parent_priority = 5;
        image.priority = 25;
    }
    (objects, handles)
}

fn position(objects: &Objects, handle: u32) -> f32 {
    let ObjectState::CImage(image) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    assert_eq!(image.parent_priority, 5);
    image.position[0]
}

#[test]
fn new_motion_keeps_the_captured_base_while_the_object_or_a_descendant_is_active() {
    for budget in [1, 4096] {
        let (mut objects, handles) = setup();
        let [parent, child] = handles[..] else {
            panic!()
        };
        let mut host = MotionHost::default();
        objects
            .start_motion(parent, movement(true), &[2.], 0)
            .unwrap();
        while !objects.update_motions(100, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 12.);
        assert!(objects.get(parent).unwrap().motions().is_empty());
        assert!(!objects.motion_active(parent).unwrap());

        objects
            .start_motion(child, movement(false), &[1.], 0)
            .unwrap();
        let ObjectState::CImage(image) = &mut objects.get_mut(parent).unwrap().state else {
            panic!()
        };
        image.position[0] = 100.;
        assert!(objects.motion_active(parent).unwrap());
        assert_eq!(
            objects
                .start_motion(parent, movement(false), &[3.], 0)
                .unwrap(),
            2
        );
        while !objects.update_motions(101, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 13.);
        assert_eq!(position(&objects, child), 21.);
        let ObjectState::CImage(image) = &mut objects.get_mut(child).unwrap().state else {
            panic!()
        };
        image.updates = Default::default();
        objects
            .start_motion(parent, movement(false), &[4.], 0)
            .unwrap();
        while !objects.update_motions(102, &mut host, budget).unwrap() {}
        assert_eq!(position(&objects, parent), 17.);
        assert_eq!(position(&objects, child), 21.);
        let ObjectState::CImage(image) = &objects.get(child).unwrap().state else {
            panic!()
        };
        assert!(image.updates.is_dirty());
    }
}

#[test]
fn motion_traversal_consumes_random_child_first_and_freezes_completed_objects_across_yields() {
    let mut code = Vec::new();
    ins(&mut code, 0x2b, &[(3, 39), (3, 0)]);
    ins(&mut code, 0x15, &[(3, 0), (0x43, 0)]);
    code.extend([0x2d, 0x3b]);
    let program = Program::from_code(&code, vec![0; 4], 64).unwrap();
    for budget in [0, 1, 4096] {
        let (mut objects, handles) = setup();
        let mut host = MotionHost::default();
        let mut expected = MotionHost::default();
        let values = [
            expected.random.next_integer(),
            expected.random.next_integer(),
        ];
        for &handle in &handles {
            objects
                .start_motion(handle, program.clone(), &[], 0)
                .unwrap();
        }
        let mut complete = objects.update_motions(100, &mut host, budget).unwrap();
        if budget <= 1 {
            assert!(!complete);
            assert!(objects.remove(handles[0]).is_err());
            assert!(objects.set_parent(handles[1], None).is_err());
            assert!(
                objects
                    .start_motion(handles[0], program.clone(), &[], 0)
                    .is_err()
            );
        }
        for _ in 0..16 {
            if complete {
                break;
            }
            complete = objects.update_motions(999, &mut host, budget).unwrap();
        }
        assert!(complete);
        for (&handle, expected) in handles.iter().rev().zip(values) {
            let queue = objects.get(handle).unwrap().motions();
            assert_eq!(queue.time_origin(1), Some(100));
            let task = queue.task(1).unwrap();
            assert_eq!(task.state().unwrap().elapsed, 0);
            assert_eq!(task.interpreter.read(0, 4).unwrap(), expected.to_le_bytes());
        }
        assert_eq!(host.random.next_integer(), expected.random.next_integer());
    }
}
