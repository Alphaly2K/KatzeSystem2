use katze_system2::{
    Error, Result, Step,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    graphics::{DrawingContext, GraphicsFactory, Projection, RenderTarget, Transform},
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Objects},
        kind,
    },
};
use std::{cell::RefCell, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂拒绝创建"))
    }
}

fn setup() -> (Runtime<GraphicsFactory<Reject>>, Objects) {
    let runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(8),
        GraphicsFactory { fallback: Reject },
    );
    let objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    (runtime, objects)
}

fn define(objects: &mut Objects, name: &str) -> u32 {
    objects
        .define(&ObjectDefinition {
            kind: "CPLANE",
            name: name.into(),
            banks: 1,
        })
        .unwrap()[0]
}

fn initialize(objects: &mut Objects, name: &str, attributes: ObjectAttributes) {
    objects
        .initialize(
            &ObjectInitialization {
                target: ObjectReference {
                    name: name.into(),
                    bank: 0,
                },
                attributes,
            },
            |_| unreachable!(),
        )
        .unwrap();
}

fn plane(objects: &Objects, handle: u32) -> &fes::CPlaneState {
    let ObjectState::CPlane(state) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    state
}

#[test]
fn motion_color_scale_and_matrix_reach_plane_drawables_without_changing_priority_or_vram() {
    for kind in ["CPLANE", "PLANE"] {
        for budget in [1, 4096] {
            let (mut runtime, mut objects) = setup();
            runtime.drawing_mut().target = Some(RenderTarget::new([8, 8]).unwrap());
            let handle = objects
                .define(&ObjectDefinition {
                    kind,
                    name: "panel".into(),
                    banks: 1,
                })
                .unwrap()[0];
            let child = define(&mut objects, "child");
            objects.set_parent(child, Some(handle)).unwrap();
            initialize(
                &mut objects,
                "panel",
                ObjectAttributes {
                    size: Some([1, 1]),
                    priority: Some(7),
                    display: Some(1),
                    base: Some([0, 0, 999]),
                    color: Some(0xff112233u32 as i32),
                    vram: Some([3, 4]),
                    ..Default::default()
                },
            );
            runtime.update_fes_object(0, &mut objects, handle).unwrap();
            runtime.update_fes_object(0, &mut objects, child).unwrap();
            assert!(!plane(&objects, child).is_dirty());
            let mut code = Vec::new();
            for (index, value) in [
                (5, 123f32.to_bits()),
                (6, 2f32.to_bits()),
                (7, 1f32.to_bits()),
                (21, 2f32.to_bits()),
                (25, 2f32.to_bits()),
                (28, 0xff445566),
                (31, 1.25f32.to_bits()),
            ] {
                code.extend([0x15, 3]);
                code.extend((0x80000000u32 + index * 4).to_le_bytes());
                code.push(3);
                code.extend(value.to_le_bytes());
            }
            code.extend([0x2d, 0x3b]);
            objects
                .start_motion(
                    handle,
                    katze_system2::kcs::Program::from_code(&code, vec![], 64).unwrap(),
                    &[],
                    0,
                )
                .unwrap();
            let mut host = katze_system2::kcs::MotionHost::default();
            while !objects.update_motions(100, &mut host, budget).unwrap() {}
            assert!(plane(&objects, child).is_dirty());
            let state = match &objects.get(handle).unwrap().state {
                ObjectState::CPlane(state) => &**state,
                ObjectState::Plane(state) => {
                    assert_eq!(state.vram(), [3., 4., 0., 0.]);
                    &state.base
                }
                _ => unreachable!(),
            };
            assert_eq!(state.priority(), 7);
            assert_eq!(state.base_point(), [0., 0., 123., 0.]);
            assert_eq!(state.rotation(), [0., 0., 1.25, 1.]);
            assert_eq!(state.color(), 0xff445566);
            runtime.update_fes_object(0, &mut objects, handle).unwrap();
            let task = objects.get(handle).unwrap().task_id().unwrap();
            let drawable = runtime.drawables().get(task).unwrap();
            assert_eq!(drawable.transform().position(), [2., 1., 0.]);
            assert_eq!(drawable.transform().base_point(), [0., 0., 7.]);
            assert_eq!(drawable.transform().scale(), [2., 1., 1.]);
            let mut matrix = *Transform::default().matrix();
            matrix[12] = 2.;
            assert_eq!(drawable.transform().matrix_override(), Some(&matrix));
            assert_eq!(drawable.color(), (0xff445566, 1));
            if kind == "CPLANE" {
                *runtime.drawing_mut() = DrawingContext {
                    enabled: true,
                    active: true,
                    target: Some(RenderTarget::new([8, 4]).unwrap()),
                    ..Default::default()
                };
                runtime
                    .render_triangles(&Projection::default(), *Transform::default().matrix())
                    .unwrap();
                let target = runtime.drawing().target.as_ref().unwrap();
                for y in 0..4 {
                    for x in 0..8 {
                        assert_eq!(
                            target.pixel(x, y),
                            Some(if (4..6).contains(&x) && y == 1 {
                                0xff445566
                            } else {
                                0
                            })
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fes_events_create_real_planes_and_parent_changes_reach_pixels_and_deletion() {
    let source = b"#DEFINE\nCPLANE group\nCPLANE panel[2]\n#OBJECT POS BASE\ngroup 1,2 9,9,9\n#OBJECT PARENT POS BASE SIZE COLOR PRI DISP\npanel group 2,1 1,1,900 2,2 0xff123456 10 1\n#START\nexit 0\n";
    for budget in [1, 100] {
        let (mut runtime, mut objects) = setup();
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        objects.publish_bindings(&mut interpreter);
        loop {
            match interpreter.run(runtime.numbers_mut(), budget).unwrap() {
                Step::Yield => (),
                Step::Event(fes::Event::CreateObject(definition)) => {
                    objects.define(&definition).unwrap();
                    objects.publish_bindings(&mut interpreter);
                    interpreter.resume();
                }
                Step::Event(fes::Event::InitializeObject(request)) => {
                    objects.initialize(&request, |_| unreachable!()).unwrap();
                    interpreter.resume();
                }
                Step::Event(fes::Event::Exit(0)) => break,
                other => panic!("{other:?}"),
            }
        }
        let group = objects.lookup("group", 0).unwrap();
        let first = objects.lookup("panel", 0).unwrap();
        let second = objects.lookup("panel", 1).unwrap();
        for handle in [group, first, second] {
            runtime.update_fes_object(0, &mut objects, handle).unwrap();
        }
        assert_eq!(objects.get(group).unwrap().task_id(), None);
        let first_task = objects.get(first).unwrap().task_id().unwrap();
        let second_task = objects.get(second).unwrap().task_id().unwrap();
        assert_eq!(runtime.tree().node(first_task).unwrap().parent, Some(0));
        let transform = runtime.drawables().get(first_task).unwrap().transform();
        assert_eq!(transform.position(), [3., 3., 0.]);
        assert_eq!(transform.base_point(), [1., 1., 10.]);
        assert_eq!(
            runtime
                .drawables()
                .get(second_task)
                .unwrap()
                .transform()
                .base_point()[2],
            11.
        );
        *runtime.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(RenderTarget::new([8, 8]).unwrap()),
        };
        let parent = *Transform::default().matrix();
        assert!(
            runtime
                .render_triangles(&Projection::default(), parent)
                .unwrap()
        );
        let target = runtime.drawing().target.as_ref().unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    target.pixel(x, y),
                    Some(if (2..4).contains(&x) && (2..4).contains(&y) {
                        0xff123456
                    } else {
                        0
                    })
                );
            }
        }
        initialize(
            &mut objects,
            "group",
            ObjectAttributes {
                position: [Some(3), None, None],
                ..Default::default()
            },
        );
        assert!(plane(&objects, first).is_dirty());
        assert!(plane(&objects, second).is_dirty());
        runtime.update_fes_object(0, &mut objects, first).unwrap();
        assert_eq!(
            runtime
                .drawables()
                .get(first_task)
                .unwrap()
                .transform()
                .position(),
            [5., 3., 0.]
        );
        runtime.remove_fes_object(0, &mut objects, group).unwrap();
        assert!(objects.lookup("panel", 0).is_none());
        for id in [first_task, second_task] {
            assert!(runtime.drawables().get(id).is_none());
            assert_eq!(runtime.object(id).unwrap().control().deletion_requested, 1);
        }
        runtime.update(1, 1).unwrap();
        assert!(runtime.object(first_task).is_none());
        assert!(runtime.object(second_task).is_none());
    }
}

struct Metadata(Rc<RefCell<Vec<Vec<u8>>>>);
impl ResourceServices for Metadata {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Ok([name, b".hg3"].concat())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.0.borrow_mut().push(name.to_vec());
        if name == b"missing.hg3" {
            return Ok(None);
        }
        let mut data = b"HG-3".to_vec();
        for word in [12u32, 0x300, 0, 3] {
            data.extend(word.to_le_bytes());
        }
        data.extend(b"stdinfo\0");
        for word in [0u32, 40, 3, 2, 32, 2, 4, 8, 8, 0, 0, 0] {
            data.extend(word.to_le_bytes());
        }
        Ok(Some(data))
    }
}

#[test]
fn rebuild_and_metadata_keep_separate_update_flags_and_one_shot_position() {
    let (mut runtime, mut objects) = setup();
    let handle = define(&mut objects, "panel");
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().task_id(), None);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size: Some([4, 6]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    let first = objects.get(handle).unwrap().task_id().unwrap();
    assert_ne!(first, 0);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(2), Some(3), None],
            display: Some(0),
            color: Some(0x80112233u32 as i32),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(first)
            .unwrap()
            .transform()
            .position(),
        [2., 3., 0.]
    );
    assert_eq!(runtime.drawables().get(first).unwrap().display(), 0);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size: Some([4, 6]),
            ..Default::default()
        },
    );
    assert!(!plane(&objects, handle).is_dirty());
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    let second = objects.get(handle).unwrap().task_id().unwrap();
    assert_ne!(first, second);
    assert!(runtime.drawables().get(first).is_none());
    assert_eq!(
        runtime
            .drawables()
            .get(second)
            .unwrap()
            .transform()
            .position(),
        [0.; 3]
    );
    assert_eq!(runtime.drawables().get(second).unwrap().display(), 0);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            object_type: Some(2),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().task_id(), Some(second));
    assert!(runtime.drawables().get(second).is_some());
    let reads = Rc::new(RefCell::new(Vec::new()));
    runtime.set_resources(Metadata(reads.clone()));
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            object_type: Some(1),
            size_resource: Some(b"size:3".to_vec()),
            position_resource: Some(b"pos:3".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    let current = objects.get(handle).unwrap().task_id().unwrap();
    assert_eq!(plane(&objects, handle).size(), [3, 2]);
    assert_eq!(plane(&objects, handle).position(), [2., 4., 0., 1.]);
    assert!(plane(&objects, handle).position_resource().is_none());
    assert!(plane(&objects, handle).size_resource().is_some());
    assert_eq!(
        &*reads.borrow(),
        &[b"size.hg3".to_vec(), b"pos.hg3".to_vec()]
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().task_id(), Some(current));
    assert_eq!(reads.borrow().len(), 2);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [None, None, Some(99)],
            size: Some([3, 2]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(plane(&objects, handle).position()[2], 0.);
    assert_eq!(reads.borrow().len(), 3);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size_resource: Some(b"missing:0".to_vec()),
            position_resource: Some(b"pos:99".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(plane(&objects, handle).size(), [0; 2]);
    assert_eq!(plane(&objects, handle).position(), [0., 0., 0., 1.]);
}

#[test]
fn failed_creation_clears_rebuild_and_viewport_reports_the_real_boundary() {
    let (mut runtime, mut objects) = setup();
    let handle = define(&mut objects, "panel");
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size: Some([1, 1]),
            color: Some(-1),
            ..Default::default()
        },
    );
    let mut rejected = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        Reject,
    );
    assert!(
        rejected
            .update_fes_object(0, &mut objects, handle)
            .unwrap_err()
            .message
            .contains("拒绝")
    );
    assert_eq!(objects.get(handle).unwrap().task_id(), None);
    assert!(!plane(&objects, handle).needs_rebuild());
    assert!(plane(&objects, handle).is_dirty());
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size: Some([1, 1]),
            viewport: Some(ObjectReference {
                name: "_TOPOBJ_".into(),
                bank: 0,
            }),
            ..Default::default()
        },
    );
    assert!(
        runtime
            .update_fes_object(0, &mut objects, handle)
            .unwrap_err()
            .message
            .contains("89")
    );
    let task = objects.get(handle).unwrap().task_id().unwrap();
    assert_ne!(task, 0);
    assert!(runtime.drawables().get(task).is_some());
    assert!(plane(&objects, handle).is_dirty());
    assert!(runtime.update_fes_object(99, &mut objects, handle).is_err());
    assert!(runtime.remove_fes_object(99, &mut objects, handle).is_err());
    let top = objects.top();
    assert!(runtime.remove_fes_object(0, &mut objects, top).is_err());
    runtime.remove_fes_object(0, &mut objects, handle).unwrap();
    assert!(runtime.drawables().get(task).is_none());

    let mut runtime = Runtime::new(
        7,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Reject },
    );
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let handle = define(&mut objects, "zero_task");
    initialize(
        &mut objects,
        "zero_task",
        ObjectAttributes {
            size: Some([1, 1]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(7, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().task_id(), Some(0));
    assert!(runtime.drawables().get(0).is_some());
    runtime.remove_fes_object(7, &mut objects, handle).unwrap();
    assert!(runtime.drawables().get(0).is_none());
}
