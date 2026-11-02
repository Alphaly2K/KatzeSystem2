use katze_system2::{
    HandleAllocator,
    fes::{KeyBlock, ObjectDefinition, ObjectReference},
    runtime::fes::{ObjectState, Objects},
};

fn add(objects: &mut Objects, name: &str, at: [i32; 3], position: [f32; 2], enabled: i32) -> u32 {
    let handle = objects
        .define(&ObjectDefinition {
            kind: "BUTTON",
            name: name.into(),
            banks: 1,
        })
        .unwrap()[0];
    let ObjectState::Button(button) = &mut objects.get_mut(handle).unwrap().state else {
        unreachable!()
    };
    button.keyblock = at;
    button.enabled = enabled;
    button.image.base.position = position;
    handle
}

fn target(name: &str) -> ObjectReference {
    ObjectReference {
        name: name.into(),
        bank: 0,
    }
}

#[test]
fn grid_cells_use_name_order_skip_disabled_first_bindings_and_ignore_explicit_keys() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    let source = add(&mut objects, "source", [0, 1, 1], [10., 10.], 1);
    add(&mut objects, "z_direct", [0, 1, 0], [10., 0.], 1);
    add(&mut objects, "a_disabled", [0, 1, 0], [10., 0.], 0);
    add(&mut objects, "a_disabled", [0, 1, 0], [10., 0.], 1);
    let left = add(&mut objects, "left", [0, 0, 0], [0., 0.], 2);
    add(&mut objects, "right", [0, 2, 0], [20., 0.], -1);
    let ObjectState::Button(button) = &mut objects.get_mut(source).unwrap().state else {
        unreachable!()
    };
    button.keys[0] = target("right");
    let blocks = [KeyBlock {
        size: [3, 3],
        ..Default::default()
    }];
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 0)
            .unwrap(),
        Some(target("left"))
    );
    let ObjectState::Button(button) = &mut objects.get_mut(left).unwrap().state else {
        unreachable!()
    };
    button.enabled = 0;
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 0)
            .unwrap(),
        Some(target("right"))
    );
    let ObjectState::Button(button) = &mut objects.get_mut(source).unwrap().state else {
        unreachable!()
    };
    button.keyblock[0] = -1;
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 0)
            .unwrap(),
        None
    );
}

#[test]
fn horizontal_fallback_preserves_the_baseline_row_scan_and_can_select_the_source() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    add(&mut objects, "source", [0, 2, 1], [20., 10.], 1);
    add(&mut objects, "above_left", [0, 1, 0], [10., 0.], 1);
    add(&mut objects, "below_left", [0, 1, 2], [10., 20.], 1);
    let blocks = [KeyBlock {
        size: [3, 3],
        ..Default::default()
    }];
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 2)
            .unwrap(),
        Some(target("source"))
    );
}

#[test]
fn neighboring_grids_skip_empty_blocks_and_choose_nearest_integer_positions_with_name_ties() {
    let mut objects = Objects::new(HandleAllocator::default()).unwrap();
    add(&mut objects, "source", [0, 0, 0], [10.9, 0.], 1);
    add(&mut objects, "z_first", [2, 1, 0], [9.1, 0.], 1);
    let second = add(&mut objects, "a_second", [2, 0, 0], [11.9, 0.], -1);
    let blocks = [
        KeyBlock {
            size: [1, 1],
            neighbors: [-1, -1, -1, 1],
        },
        KeyBlock {
            size: [1, 1],
            neighbors: [-1, -1, -1, 2],
        },
        KeyBlock {
            size: [2, 1],
            ..Default::default()
        },
    ];
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 3)
            .unwrap(),
        Some(target("a_second"))
    );
    let ObjectState::Button(button) = &mut objects.get_mut(second).unwrap().state else {
        unreachable!()
    };
    button.enabled = 0;
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 3)
            .unwrap(),
        Some(target("z_first"))
    );
    // 这对平方距离在开平方后会舍入为同一个 f32。
    for (name, position) in [
        ("source", [0., 0.]),
        ("a_second", [4500., 4503.]),
        ("z_first", [4501., 4502.]),
    ] {
        let handle = objects.lookup(name, 0).unwrap();
        let ObjectState::Button(button) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        button.enabled = 1;
        button.image.base.position = position;
    }
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 3)
            .unwrap(),
        Some(target("z_first"))
    );
    for (name, x) in [("a_second", 10001.), ("z_first", 10000.)] {
        let handle = objects.lookup(name, 0).unwrap();
        let ObjectState::Button(button) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        button.image.base.position = [x, 0.];
    }
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 3)
            .unwrap(),
        None
    );
    let handle = objects.lookup("z_first", 0).unwrap();
    let ObjectState::Button(button) = &mut objects.get_mut(handle).unwrap().state else {
        unreachable!()
    };
    button.image.base.position[0] = 9999.;
    assert_eq!(
        objects
            .navigate_button(&blocks, &target("source"), 3)
            .unwrap(),
        Some(target("z_first"))
    );
    assert!(
        objects
            .navigate_button(&blocks, &target("source"), 4)
            .is_err()
    );
}

#[test]
fn scene_preparation_loads_only_the_first_keyblock_table_once_before_start() {
    use katze_system2::{expression::State, fes::Program, runtime::fes::Scene};
    let source = b"#KEYBLOCK 1\n0 ($0+=2),1 -1,-1,-1,-1\n#KEYBLOCK 9\n0 ($0+=9),9\n#DEFINE\nBUTTON source\nBUTTON next\n#OBJECT KEYBLOCK\nsource 0,0,0\nnext 0,1,0\n#START\n$1=$1+1\nwait 99\n";
    let mut scene = Scene::new(
        Program::from_text(source).unwrap(),
        HandleAllocator::default(),
    )
    .unwrap();
    let mut numbers = State::new(8);
    let mut complete = false;
    for _ in 0..64 {
        if scene.prepare(&mut numbers, 1).unwrap() {
            complete = true;
            break;
        }
    }
    assert!(complete);
    assert_eq!(&numbers.shared[..2], &[2, 0]);
    assert_eq!(
        scene.keyblocks(),
        &[KeyBlock {
            size: [2, 1],
            ..Default::default()
        }]
    );
    assert_eq!(
        scene
            .objects()
            .navigate_button(scene.keyblocks(), &target("source"), 3)
            .unwrap(),
        Some(target("next"))
    );
    assert!(scene.prepare(&mut numbers, 1).unwrap());
    assert_eq!(&numbers.shared[..2], &[2, 0]);
}
