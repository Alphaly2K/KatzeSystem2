use katze_system2::{
    Error, Result, Step,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    graphics::{GraphicsFactory, Projection, Rect, RenderTarget, TextureFilter, Transform},
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, Runtime, TaskObject,
        fes::{ObjectState, Objects},
        kind,
    },
    task::Command,
};
use std::{cell::RefCell, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

type TestRuntime = Runtime<GraphicsFactory<Reject>>;

fn send(runtime: &mut TestRuntime, task: u32, code: u32, a: u32, b: u32) -> u32 {
    runtime
        .dispatch_command(
            0,
            Command {
                target: task,
                sender: 0,
                code,
                data: [a, b, 0],
            },
        )
        .unwrap()
}

fn setup() -> (TestRuntime, Objects, u32) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Reject },
    );
    runtime.host_mut().surface_size = [8, 6];
    let drawing = runtime.drawing_mut();
    drawing.enabled = true;
    drawing.active = true;
    drawing.target = Some(RenderTarget::new([8, 6]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut runtime, list, 285, 8, 6);
    send(&mut runtime, list, 289, 0, 0);
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    objects.set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
    (runtime, objects, list)
}

fn define(objects: &mut Objects, kind: &'static str, name: &str) -> u32 {
    objects
        .define(&ObjectDefinition {
            kind,
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

fn image(objects: &Objects, handle: u32) -> &fes::CImageState {
    let ObjectState::CImage(state) = &objects.get(handle).unwrap().state else {
        panic!()
    };
    state
}

fn frame(runtime: &mut TestRuntime, budget: usize) -> RenderTarget {
    runtime.set_budget(budget, 1000);
    runtime.update(0, 0).unwrap();
    let mut output = RenderTarget::new([8, 6]).unwrap();
    runtime
        .render_frame(
            &mut output,
            &Projection::default(),
            *Transform::default().matrix(),
            TextureFilter::Point,
        )
        .unwrap();
    output
}

fn assert_rectangle(target: &RenderTarget, rectangle: Rect, color: u32) {
    for y in 0..6 {
        for x in 0..8 {
            let inside = x >= rectangle.left
                && x < rectangle.right
                && y >= rectangle.top
                && y < rectangle.bottom;
            assert_eq!(
                target.pixel(x as u32, y as u32),
                Some(if inside { color } else { 0 }),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn fes_cimage_events_reach_shared_canvas_final_frame_and_subtree_cleanup() {
    for budget in [1, 100] {
        let (mut runtime, mut objects, list) = setup();
        let source = b"#DEFINE\nCIMAGE group\nCIMAGE swatch\nPLANE panel\n#OBJECT POS\ngroup 2,1\n#OBJECT PARENT SIZE COLOR PRI DISP\nswatch group 3,2 0xff123456 7 1\n#OBJECT POS SIZE VRAM PRI DISP\npanel 0,0 8,6 0,0 500 1\n#START\nexit 0\n";
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
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
                Step::Event(fes::Event::Exit(0)) => interpreter.resume(),
                Step::Halted => break,
                other => panic!("{other:?}"),
            }
        }
        let group = objects.lookup("group", 0).unwrap();
        let swatch = objects.lookup("swatch", 0).unwrap();
        let panel = objects.lookup("panel", 0).unwrap();
        for handle in [group, swatch, panel] {
            runtime.update_fes_object(0, &mut objects, handle).unwrap();
        }
        let wrapper = objects.get(swatch).unwrap().image_handle().unwrap();
        let shared = runtime.images_mut().share(wrapper).unwrap();
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
        assert_rectangle(
            &frame(&mut runtime, budget),
            Rect::from_xywh(2, 1, 3, 2),
            0xff12_3456,
        );
        runtime
            .images_mut()
            .get_mut(wrapper)
            .unwrap()
            .fill_rect_from_bottom(Rect::from_xywh(0, 0, 3, 2), 0xff77_8899);
        runtime.update_fes_object(0, &mut objects, swatch).unwrap();
        assert_rectangle(
            &frame(&mut runtime, budget),
            Rect::from_xywh(2, 1, 3, 2),
            0xff12_3456,
        );
        initialize(
            &mut objects,
            "swatch",
            ObjectAttributes {
                color: Some(0xff12_3456u32 as i32),
                ..Default::default()
            },
        );
        runtime.update_fes_object(0, &mut objects, swatch).unwrap();
        assert_eq!(
            runtime.images().get(shared).unwrap().surface().pixel(0, 0),
            Some(0xff12_3456)
        );
        initialize(
            &mut objects,
            "group",
            ObjectAttributes {
                position: [Some(3), Some(2), None],
                ..Default::default()
            },
        );
        assert!(image(&objects, swatch).updates.is_dirty());
        for handle in [group, swatch] {
            runtime.update_fes_object(0, &mut objects, handle).unwrap();
        }
        assert_rectangle(
            &frame(&mut runtime, budget),
            Rect::from_xywh(3, 2, 3, 2),
            0xff12_3456,
        );
        let ObjectState::CImage(state) = &mut objects.get_mut(swatch).unwrap().state else {
            panic!()
        };
        state.base_point = [1., 1.];
        objects.invalidate(swatch).unwrap();
        runtime.update_fes_object(0, &mut objects, swatch).unwrap();
        assert_rectangle(
            &frame(&mut runtime, budget),
            Rect::from_xywh(1, 0, 3, 2),
            0xff12_3456,
        );
        runtime.remove_fes_object(0, &mut objects, group).unwrap();
        assert!(runtime.images().get(wrapper).is_err());
        assert!(runtime.images().get(shared).is_ok());
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
        assert_rectangle(&frame(&mut runtime, budget), Rect::default(), 0);
        runtime.images_mut().release(shared).unwrap();
    }
}

#[test]
fn plane_binding_clips_the_canvas_source_before_the_plane_samples_it() {
    let (mut runtime, mut objects, _) = setup();
    let panel = define(&mut objects, "PLANE", "panel");
    let swatch = define(&mut objects, "CIMAGE", "swatch");
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(3), Some(3), None],
            size: Some([3, 2]),
            vram: Some([2, 1]),
            display: Some(1),
            ..Default::default()
        },
    );
    initialize(
        &mut objects,
        "swatch",
        ObjectAttributes {
            plane: Some(ObjectReference {
                name: "panel".into(),
                bank: 0,
            }),
            size: Some([5, 4]),
            color: Some(0xff11_2233u32 as i32),
            priority: Some(7),
            display: Some(1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, swatch).unwrap();
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(3, 3, 3, 2),
        0xff11_2233,
    );
    assert_rectangle(
        runtime.drawing().target.as_ref().unwrap(),
        Rect::from_xywh(2, 1, 3, 2),
        0xff11_2233,
    );
}

struct Metadata(Rc<RefCell<Vec<Vec<u8>>>>);
impl ResourceServices for Metadata {
    fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Ok(name.to_vec())
    }
    fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.0.borrow_mut().push(name.to_vec());
        let mut data = b"HG-3".to_vec();
        for word in [12u32, 0x300, 0, 3] {
            data.extend(word.to_le_bytes());
        }
        data.extend(b"stdinfo\0");
        for word in [0u32, 40, 3, 2, 32, 2, 1, 8, 6, 0, 0, 0] {
            data.extend(word.to_le_bytes());
        }
        Ok(Some(data))
    }
}

#[test]
fn cimage_metadata_waits_for_rebuild_and_empty_sources_keep_failure_flags() {
    let (mut runtime, mut objects, list) = setup();
    let swatch = define(&mut objects, "CIMAGE", "swatch");
    let reads = Rc::new(RefCell::new(Vec::new()));
    runtime.set_resources(Metadata(reads.clone()));
    initialize(
        &mut objects,
        "swatch",
        ObjectAttributes {
            size_resource: Some(b"dimensions:3".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, swatch).unwrap();
    let wrapper = objects.get(swatch).unwrap().image_handle().unwrap();
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert!(reads.borrow().is_empty());
    initialize(
        &mut objects,
        "swatch",
        ObjectAttributes {
            color: Some(-1),
            position_resource: Some(b"position:3".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, swatch).unwrap();
    assert_eq!(image(&objects, swatch).size, [3, 2]);
    assert_eq!(image(&objects, swatch).position, [2., 1.]);
    assert_eq!(
        &*reads.borrow(),
        &[b"dimensions".to_vec(), b"position".to_vec()]
    );
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
    runtime.update_fes_object(0, &mut objects, swatch).unwrap();
    assert_eq!(reads.borrow().len(), 2);
    let failed = define(&mut objects, "CIMAGE", "failed");
    initialize(
        &mut objects,
        "failed",
        ObjectAttributes {
            size: Some([-2, -2]),
            display: Some(1),
            ..Default::default()
        },
    );
    assert!(runtime.update_fes_object(0, &mut objects, failed).is_err());
    assert!(!image(&objects, failed).updates.needs_rebuild());
    assert!(image(&objects, failed).updates.is_dirty());
    let empty = objects.get(failed).unwrap().image_handle().unwrap();
    assert_eq!(runtime.images().pixel_handle(empty).unwrap(), 0);
    runtime.update_fes_object(0, &mut objects, failed).unwrap();
    assert!(!image(&objects, failed).updates.is_dirty());
    initialize(
        &mut objects,
        "failed",
        ObjectAttributes {
            size: Some([0, 0]),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, failed).unwrap();
    assert_ne!(runtime.images().pixel_handle(empty).unwrap(), 0);
    objects.set_image_list(0, Rect::default());
    assert!(runtime.update_fes_object(0, &mut objects, swatch).is_err());
}
