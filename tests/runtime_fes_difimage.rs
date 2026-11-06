use katze_system2::{
    Error, Result,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    graphics::{
        Drawable, GraphicsFactory, ImageFilter, ImageList, ImageListProperties, Images, Projection,
        Rect, RenderTarget, TextureFilter, Transform,
    },
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Objects, Scene},
        kind,
    },
    task::Command,
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
type Engine = Runtime<GraphicsFactory<Reject>>;
fn engine() -> (Engine, Objects) {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(4),
        GraphicsFactory { fallback: Reject },
    );
    rt.host_mut().surface_size = [8, 6];
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().active = true;
    rt.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    rt.set_frame_view(
        Projection {
            translation: [1., 0.],
            ..Projection::default()
        },
        *Transform::default().matrix(),
        TextureFilter::Point,
    );
    let list = rt.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    rt.dispatch_command(
        0,
        Command {
            target: list,
            sender: 0,
            code: 285,
            data: [8, 6, 0],
        },
    )
    .unwrap();
    let mut objects = Objects::new(rt.host().handle_allocator()).unwrap();
    objects.set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
    (rt, objects)
}
fn command(rt: &mut Engine, objects: &mut Objects, line: &str) {
    let mut vm =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 999\n").unwrap()).unwrap();
    objects.publish_bindings(&mut vm);
    assert!(
        objects
            .dispatch_object_command(
                &mut vm,
                &fes::Line {
                    offset: 0,
                    bytes: line.as_bytes().to_vec()
                },
                rt.numbers_mut()
            )
            .unwrap()
    );
}
fn layer(rt: &mut Engine, id: u32, depth: f32, size: [f32; 2], color: u32) {
    rt.drawables_mut()
        .insert(id, Drawable::rectangle([0.; 3], size, color))
        .unwrap();
    rt.drawables_mut().set_display(id, 1);
    rt.drawables_mut().set_base_point(id, [0., 0., depth]);
}

#[test]
fn capture_uses_saved_view_depth_bounds_crop_and_replaces_sources() {
    let (mut rt, mut objects) = engine();
    let handle = objects
        .define(&ObjectDefinition {
            kind: "DIFIMAGE",
            name: "item".into(),
            banks: 1,
        })
        .unwrap()[0];
    objects
        .initialize(
            &ObjectInitialization {
                target: ObjectReference {
                    name: "item".into(),
                    bank: 0,
                },
                attributes: ObjectAttributes {
                    priority: Some(5),
                    display: Some(1),
                    ..ObjectAttributes::default()
                },
            },
            |_| unreachable!(),
        )
        .unwrap();
    layer(&mut rt, 100, 9., [8., 6.], 0xffff0000);
    layer(&mut rt, 101, 10., [3., 2.], 0xff123456);
    layer(&mut rt, 102, 20., [8., 6.], 0xff00ff00);
    rt.set_frame_capture_rectangle(Some(Rect::from_xywh(1, 0, 4, 3)));
    let canvas = rt.drawing().target.clone();
    command(&mut rt, &mut objects, "item refcap 10 20");
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    let source = objects.get(handle).unwrap().image_handle().unwrap();
    let ObjectState::DifImage(state) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    let resource = state.resource_handle().unwrap();
    assert!(rt.images().get(resource).is_err());
    assert_eq!(rt.image_resource(resource).unwrap().info().size, [4, 3]);
    let snapshot = rt.images().get(source).unwrap();
    assert_eq!(snapshot.info().size, [4, 3]);
    assert_eq!(snapshot.info().canvas_size, [0; 2]);
    assert_eq!(snapshot.surface().pixel(0, 0), Some(0xff123456));
    assert_eq!(snapshot.surface().pixel(3, 0), Some(0));
    assert_eq!(snapshot.surface().pixel(0, 2), Some(0));
    assert_eq!(rt.drawing().target, canvas);
    command(&mut rt, &mut objects, "item filter np");
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(
        rt.images().get(source).unwrap().surface().pixel(0, 0),
        Some(0xffedcba9)
    );
    command(&mut rt, &mut objects, "item filter NP");
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(
        rt.images().get(source).unwrap().surface().pixel(0, 0),
        Some(0xffedcba9)
    );
    let held = rt.images_mut().share(source).unwrap();
    let canvas = rt.drawing_mut().target.take();
    command(&mut rt, &mut objects, "item refcap");
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().image_handle(), Some(source));
    rt.drawing_mut().target = canvas;
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    assert_eq!(objects.get(handle).unwrap().image_handle(), Some(source));
    command(&mut rt, &mut objects, "item refcap 10 10");
    rt.update_fes_object(0, &mut objects, handle).unwrap();
    let second = objects.get(handle).unwrap().image_handle().unwrap();
    assert_ne!(source, second);
    assert!(rt.image_resource(resource).is_err());
    assert!(rt.images().get(source).is_err());
    assert!(
        rt.images()
            .get(second)
            .unwrap()
            .surface()
            .pixels()
            .iter()
            .all(|&b| b == 0)
    );
    assert_eq!(
        rt.images().get(held).unwrap().surface().pixel(0, 0),
        Some(0xffedcba9)
    );
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 99\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    assert!(
        rt.dispatch_fes_reference(
            &mut objects,
            &mut interpreter,
            &fes::Line {
                offset: 0,
                bytes: b"item refex".to_vec()
            },
        )
        .unwrap()
    );
    assert_eq!(objects.get(handle).unwrap().image_handle(), None);
    assert!(rt.images().get(second).is_ok());
    rt.remove_fes_object(0, &mut objects, handle).unwrap();
    assert!(rt.images().get(second).is_err());
    rt.images_mut().release(held).unwrap();
}

#[test]
fn capture_declarations_and_failed_requests_keep_bank_and_budget_order() {
    for budget in [1, 4096] {
        let (mut rt, bindings) = engine();
        rt.set_budget(budget, 10000);
        let source = b"#DEFINE\nDIFIMAGE item[2]\nDIFIMAGE item[2]\n#START\nitem refcap ($0=$0+1) ($0=$0+1)\nwait 999\n";
        let mut scene = Scene::new(
            fes::Program::from_text(source).unwrap(),
            rt.host().handle_allocator(),
        )
        .unwrap();
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(bindings.image_list().unwrap(), Rect::from_xywh(0, 0, 8, 6));
        while !rt.prepare_fes_scene(0, &mut scene).unwrap() {}
        assert_eq!(scene.objects().media_records().len(), 4);
        for record in scene.objects().media_records() {
            assert_eq!(record.image_handle(), None);
            assert_eq!(record.task_id(), None);
        }
        rt.drawing_mut().enabled = false;
        while !rt.update_fes_scene(0, &mut scene, 0).unwrap() {}
        assert_eq!(rt.numbers().shared[0], 2);
        for bank in 0..2 {
            let handle = scene.objects().lookup("item", bank).unwrap();
            let object = scene.objects().get(handle).unwrap();
            let ObjectState::DifImage(state) = &object.state else {
                panic!()
            };
            assert_eq!(state.pending_capture(), None);
            assert_eq!(object.image_handle(), None);
        }
        let objects = scene.objects_mut().unwrap();
        command(&mut rt, objects, "item refcap");
        for bank in 0..2 {
            let handle = objects.lookup("item", bank).unwrap();
            let ObjectState::DifImage(state) = &objects.get(handle).unwrap().state else {
                panic!()
            };
            assert_eq!(state.pending_capture(), Some([0, 45000]));
        }
        command(&mut rt, objects, "item refcap -1 -2");
        for bank in 0..2 {
            let handle = objects.lookup("item", bank).unwrap();
            rt.update_fes_object(0, objects, handle).unwrap();
        }
        rt.drawing_mut().enabled = true;
        let handle = objects.lookup("item", 0).unwrap();
        rt.update_fes_object(0, objects, handle).unwrap();
        assert_eq!(objects.get(handle).unwrap().image_handle(), None);
    }
}

#[test]
fn filtered_captures_repaint_each_list_and_keep_shared_pixels_alive() {
    let mut images = Images::default();
    let source = images.create(0, [2, 1]).unwrap();
    images
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .copy_from_slice(&[0, 0, 255, 255, 255, 0, 0, 128]);
    let mut lists = [
        ImageList::new(1, [2, 1], &mut images).unwrap(),
        ImageList::new(2, [2, 1], &mut images).unwrap(),
    ];
    for list in &mut lists {
        list.set_capture(0, source, ImageListProperties::default(), &mut images)
            .unwrap();
        list.update(&mut images).unwrap();
        assert!(list.update(&mut images).unwrap().is_empty());
    }
    ImageFilter::Mono.apply(images.get_mut(source).unwrap());
    assert_eq!(
        images.get(source).unwrap().surface().pixels(),
        &[76, 76, 76, 255, 29, 29, 29, 128]
    );
    images.release(source).unwrap();
    for list in &mut lists {
        assert!(!list.update(&mut images).unwrap().is_empty());
        assert!(list.update(&mut images).unwrap().is_empty());
        assert_eq!(
            images.get(list.image()).unwrap().surface().pixel(0, 0),
            Some(0xff4c4c4c)
        );
    }
    for list in &mut lists {
        list.release(&mut images).unwrap();
    }
}
