use katze_system2::{
    Error, Result, Step,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    graphics::{
        GraphicsFactory, PlaneObject, Projection, Rect, RenderTarget, Surface, TextureFilter,
        Transform,
    },
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

fn plane(objects: &Objects, handle: u32) -> &fes::PlaneState {
    let ObjectState::Plane(state) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    state
}

fn color(x: u32, y: u32) -> u32 {
    0xff000000 | (x << 16) | (y << 8)
}

#[test]
fn fes_plane_samples_canvas_tiles_and_keeps_source_and_screen_positions_separate() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Reject },
    );
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let source = b"#DEFINE\nPLANE panel\nCPLANE child\n#OBJECT POS BASE SIZE VRAM PRI DISP COLOR FILE\npanel 3,3 1,1,90 3,2 1,0 7 1 0xffffffff ignored\n#OBJECT PARENT POS BASE SIZE PRI DISP COLOR\nchild panel 1,1 0,0,0 1,1 20 0 0xffffffff\n#START\nexit 0\n";
    let mut interpreter = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    loop {
        match interpreter.run(runtime.numbers_mut(), 1).unwrap() {
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
    let mut tiles = Vec::new();
    for tile in 0..2 {
        let mut surface = Surface::new([4, 4]).unwrap();
        for y in 0..4 {
            for x in 0..4 {
                surface.fill_rect(
                    Rect::from_xywh(x, y, 1, 1),
                    color(x as u32 + tile * 4, y as u32),
                );
            }
        }
        tiles.push(Some(surface));
    }
    runtime.drawing_mut().target = Some(RenderTarget::from_tiles([4, 4], [2, 1], tiles).unwrap());
    objects.set_canvas_offset([1, 0]);
    let panel = objects.lookup("panel", 0).unwrap();
    let child = objects.lookup("child", 0).unwrap();
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    runtime.update_fes_object(0, &mut objects, child).unwrap();
    let task = objects.get(panel).unwrap().task_id().unwrap();
    let child_task = objects.get(child).unwrap().task_id().unwrap();
    let drawable = runtime.drawables().get(task).unwrap();
    assert_eq!(drawable.transform().position(), [3., 3., 0.]);
    assert_eq!(
        drawable.geometry_transform(true).unwrap().position(),
        [2., 0., 0.]
    );
    assert_eq!(
        runtime
            .drawables()
            .get(child_task)
            .unwrap()
            .transform()
            .base_point(),
        [1., 1., 20.]
    );
    assert_eq!(
        runtime
            .drawables()
            .get(child_task)
            .unwrap()
            .transform()
            .position(),
        [4., 4., 0.]
    );
    let mut output = RenderTarget::new([8, 8]).unwrap();
    let parent = *Transform::default().matrix();
    assert!(
        runtime
            .render_frame(
                &mut output,
                &Projection::default(),
                parent,
                TextureFilter::Point
            )
            .unwrap()
    );
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                output.pixel(x, y),
                Some(if (2..5).contains(&x) && (2..4).contains(&y) {
                    color(x, y - 2)
                } else {
                    0
                })
            );
        }
    }
    assert!(
        runtime
            .drawables()
            .get(task)
            .unwrap()
            .canvas_triangles()
            .unwrap()
            .iter()
            .any(|t| t.tile == 1)
    );
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            vram: Some([0, 1]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(task)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [1., 1., 0.]
    );
    objects.set_canvas_offset([2, 1]);
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(task)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [1., 1., 0.]
    );
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            color: Some(-1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(task)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [2., 2., 0.]
    );
    let mut output = RenderTarget::new([8, 8]).unwrap();
    runtime
        .render_frame(
            &mut output,
            &Projection::default(),
            parent,
            TextureFilter::Point,
        )
        .unwrap();
    assert_eq!(output.pixel(2, 2), Some(color(2, 2)));
    assert_eq!(output.pixel(4, 3), Some(color(4, 3)));

    let barrier = objects
        .define(&ObjectDefinition {
            kind: "CPLANE",
            name: "barrier".into(),
            banks: 1,
        })
        .unwrap()[0];
    objects.set_parent(barrier, Some(panel)).unwrap();
    initialize(
        &mut objects,
        "barrier",
        ObjectAttributes {
            position: [Some(4), Some(5), None],
            base: Some([5, 6, 0]),
            ..Default::default()
        },
    );
    objects.set_parent(child, Some(barrier)).unwrap();
    initialize(
        &mut objects,
        "child",
        ObjectAttributes {
            color: Some(-1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, child).unwrap();
    let child_transform = runtime.drawables().get(child_task).unwrap().transform();
    assert_eq!(child_transform.position(), [8., 9., 0.]);
    assert_eq!(child_transform.base_point(), [0., 0., 20.]);
    runtime.remove_fes_object(0, &mut objects, panel).unwrap();
    assert!(runtime.drawables().get(task).is_none());
    assert!(runtime.drawables().get(child_task).is_none());
}

struct RecordingFactory(Rc<RefCell<Vec<&'static str>>>);
impl ObjectFactory for RecordingFactory {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        assert_eq!(kind, kind::PLANE);
        assert_eq!(name, b"FE Plane");
        self.0.borrow_mut().push("create");
        Ok(Box::new(PlaneObject::new(id)))
    }
}

struct Metadata(Rc<RefCell<Vec<&'static str>>>);
impl ResourceServices for Metadata {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        assert_eq!(name, b"meta");
        Ok(name.to_vec())
    }
    fn read(&mut self, _: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.0.borrow_mut().push("read");
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
fn plane_rebuild_creates_before_metadata_and_preserves_its_position_after_retry() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        RecordingFactory(calls.clone()),
    );
    runtime.set_resources(Metadata(calls.clone()));
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    let handle = objects
        .define(&ObjectDefinition {
            kind: "PLANE",
            name: "panel".into(),
            banks: 1,
        })
        .unwrap()[0];
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(5), Some(6), None],
            base: Some([1, 2, 90]),
            size_resource: Some(b"meta:3".to_vec()),
            position_resource: Some(b"meta:3".to_vec()),
            display: Some(1),
            ..Default::default()
        },
    );
    assert!(
        runtime
            .update_fes_object(0, &mut objects, handle)
            .unwrap_err()
            .message
            .contains("画布纹理布局")
    );
    assert_eq!(&*calls.borrow(), &["create", "read"]);
    let failed = objects.get(handle).unwrap().task_id().unwrap();
    assert!(runtime.object(failed).is_some());
    assert!(plane(&objects, handle).base.needs_rebuild());
    assert!(plane(&objects, handle).base.position_resource().is_some());
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 8]).unwrap());
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    let task = objects.get(handle).unwrap().task_id().unwrap();
    assert_ne!(failed, task);
    assert_eq!(
        runtime.object(failed).unwrap().control().deletion_requested,
        1
    );
    assert_eq!(
        &*calls.borrow(),
        &["create", "read", "create", "read", "read"]
    );
    assert_eq!(plane(&objects, handle).base.size(), [3, 2]);
    assert_eq!(plane(&objects, handle).base.position(), [2., 4., 0., 1.]);
    assert!(!plane(&objects, handle).base.needs_rebuild());
    assert!(!plane(&objects, handle).base.is_dirty());
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(calls.borrow().len(), 5);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(5), None, None],
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            size: Some([3, 2]),
            ..Default::default()
        },
    );
    assert!(!plane(&objects, handle).base.is_dirty());
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    let rebuilt = objects.get(handle).unwrap().task_id().unwrap();
    assert_ne!(task, rebuilt);
    assert_eq!(
        &*calls.borrow(),
        &["create", "read", "create", "read", "read", "create", "read"]
    );
    let drawable = runtime.drawables().get(rebuilt).unwrap();
    assert_eq!(drawable.transform().position(), [5., 4., 0.]);
    assert_eq!(drawable.transform().base_point(), [0., 0., 5000.]);
    assert_eq!(drawable.display(), 0);
    objects.set_canvas_offset([1, -1]);
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            vram: Some([i32::MAX, i32::MIN]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(rebuilt)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [-2147483648., 2147483648., 0.]
    );
}
