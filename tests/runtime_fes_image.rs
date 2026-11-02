use katze_system2::{
    Error, Result, Step,
    expression::State,
    fes::{self, ObjectAttributes, ObjectDefinition, ObjectInitialization, ObjectReference},
    graphics::{GraphicsFactory, Projection, Rect, RenderTarget, TextureFilter, Transform},
    input::InputFactory,
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, RootObject, Runtime, TaskObject,
        fes::{ObjectState, Objects},
        kind,
    },
    task::{Command, CommandBuffers},
};
use std::{cell::RefCell, collections::BTreeMap, io::Write, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

type TestRuntime = Runtime<GraphicsFactory<InputFactory<Reject>>>;

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

fn source_slot(runtime: &mut TestRuntime, list: u32, identity: u32) -> Option<u32> {
    let mut bytes = [0xa5; 4];
    let found = runtime
        .dispatch_command_with_buffers(
            0,
            Command {
                target: list,
                sender: 0,
                code: 308,
                data: [identity, 0, 0],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
            },
        )
        .unwrap();
    if found != 0 {
        Some(u32::from_le_bytes(bytes))
    } else {
        assert_eq!(bytes, [0xa5; 4]);
        None
    }
}

fn setup() -> (TestRuntime, Objects, u32) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: InputFactory { fallback: Reject },
        },
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
            |name| Ok(name.to_vec()),
        )
        .unwrap();
}

fn image(objects: &Objects, handle: u32) -> &fes::CImageState {
    match &objects.get(handle).unwrap().state {
        ObjectState::CImage(state) => state,
        ObjectState::Image(state) => &state.base,
        _ => panic!(),
    }
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
                Step::Event(fes::Event::Exit(0)) => break,
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

struct PixelFiles {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl ResourceServices for PixelFiles {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Ok(name.to_vec())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.borrow_mut().push(name.to_vec());
        Ok(self.files.get(name).cloned())
    }
}

fn pixel_file(color: u32) -> Vec<u8> {
    let compress = |bytes: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let delta = color
        .to_le_bytes()
        .map(|v| (v << 1) ^ 0u8.wrapping_sub(v >> 7));
    let planes: Vec<_> = [6, 4, 2, 0]
        .into_iter()
        .map(|shift| {
            delta
                .iter()
                .enumerate()
                .fold(0, |p, (i, v)| p | (((v >> shift) & 3) << (2 * i)))
        })
        .collect();
    let data = compress(&planes);
    let control = compress(&[9, 1]);
    let mut bytes = b"HG-3".to_vec();
    for value in [12u32, 0x300, 0, 7] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"stdinfo\0");
    for value in [56u32, 40, 1, 1, 32, 1, 2, 10, 20, 0, 3, 4] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"img0000\0");
    for value in [
        0,
        (24 + data.len() + control.len()) as u32,
        0,
        1,
        data.len() as u32,
        4,
        control.len() as u32,
        2,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(data);
    bytes.extend(control);
    bytes
}

#[test]
fn duplicate_button_image_sources_report_the_missing_record_wrapper() {
    let (mut runtime, _, list) = setup();
    let program = fes::Program::from_text(
        b"#DEFINE\nBUTTON item\nBUTTON item\n#OBJECT FILE\nitem sprite\n#START\nwait 1\n",
    )
    .unwrap();
    let mut scene =
        katze_system2::runtime::fes::Scene::new(program, runtime.host().handle_allocator())
            .unwrap();
    scene
        .objects_mut()
        .unwrap()
        .set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
    let error = runtime.update_fes_scene(0, &mut scene, 100).unwrap_err();
    assert!(
        error
            .message
            .contains("重复 BUTTON 声明的独立图像包装尚未接入")
    );
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
}

#[test]
fn fes_buttons_retry_input_after_image_loading_and_update_real_regions_and_pixels() {
    for budget in [1, 4096] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(RootObject::new(1, 0)),
            State::new(1),
            GraphicsFactory {
                fallback: InputFactory { fallback: Reject },
            },
        );
        runtime.set_budget(budget, 10000);
        runtime.host_mut().surface_size = [8, 6];
        runtime.input_coordinates_mut().logical_size = [8, 6];
        runtime.drawing_mut().enabled = true;
        runtime.drawing_mut().active = true;
        runtime.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
        let placeholder = runtime
            .create(0, kind::IMAGE_LIST, b"input placeholder", b"")
            .unwrap();
        assert_eq!(placeholder, 1);
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        assert_eq!(list, 2);
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, list, 289, 0, 0);
        let mut pixels = pixel_file(0xffff_0000);
        pixels[48..56].fill(0);
        pixels[68..76].fill(0);
        let reads = Rc::new(RefCell::new(Vec::new()));
        runtime.set_resources(PixelFiles {
            files: BTreeMap::from([(b"sprite".to_vec(), pixels)]),
            reads: reads.clone(),
        });
        let program = fes::Program::from_text(b"#DEFINE\nBUTTON item[2]\n#OBJECT FILE IDN PRI POS ENABLE TYPE DISP\nitem sprite 7 5 1,1 21 1 1\n#OBJECT POS\nitem[1] 4,1\n#START\n$0=$0+1\nwait 1\nitem[0] enable 0\nitem[1] pri 9\nwait 1\nitem[1] disp 0\nitem[0] enable -3\nitem[0] size 2 2\nwait 99\n").unwrap();
        let mut scene =
            katze_system2::runtime::fes::Scene::new(program, runtime.host().handle_allocator())
                .unwrap();
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
        let mut prepared = false;
        for _ in 0..64 {
            if runtime.prepare_fes_scene(0, &mut scene).unwrap() {
                prepared = true;
                break;
            }
        }
        assert!(prepared);
        assert_eq!(runtime.numbers().shared[0], 0);
        assert!(reads.borrow().is_empty());
        let handles = [
            scene.objects().lookup("item", 0).unwrap(),
            scene.objects().lookup("item", 1).unwrap(),
        ];
        let tasks = handles.map(|id| scene.objects().get(id).unwrap().task_id().unwrap());
        assert_eq!(runtime.tree().children(0).unwrap().count(), 4);
        assert!(runtime.prepare_fes_scene(0, &mut scene).unwrap());
        assert_eq!(runtime.tree().children(0).unwrap().count(), 4);

        // Root 的输入编号先指向列表；失败发生在 IMAGE 更新完成之后。
        let mut failed = false;
        for _ in 0..64 {
            match runtime.update_fes_scene(0, &mut scene, 100) {
                Ok(false) => (),
                Err(error) => {
                    assert!(error.message.contains("输入任务绑定失效"));
                    failed = true;
                    break;
                }
                Ok(true) => panic!("未诊断错误的输入绑定"),
            }
        }
        assert!(failed);
        assert_eq!(&*reads.borrow(), &[b"sprite".to_vec()]);
        assert_eq!(runtime.numbers().shared[0], 1);
        // 替换错误类型的服务，保留输入编号和场景续跑。
        runtime.destroy(placeholder).unwrap();
        runtime.update(0, 0).unwrap();
        assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
        assert!(scene.is_pending());
        for now in [100, 101, 102] {
            let mut complete = false;
            for _ in 0..64 {
                if runtime.update_fes_scene(0, &mut scene, now).unwrap() {
                    complete = true;
                    break;
                }
            }
            assert!(complete);
            assert_eq!(runtime.numbers().shared[0], 1);
            assert_eq!(runtime.input_state(1).unwrap().regions().records().len(), 2);
            assert_eq!(send(&mut runtime, list, 294, 0, 0), 2);
            for (bank, &task) in tasks.iter().enumerate() {
                let ObjectState::Button(button) =
                    &scene.objects().get(handles[bank]).unwrap().state
                else {
                    panic!()
                };
                assert_eq!(button.image.base.parent_priority, 5 + bank as i32);
                let region = runtime.input_state(1).unwrap().regions().get(task).unwrap();
                assert_eq!(
                    region.enabled(),
                    if bank == 0 && now == 101 {
                        0
                    } else if bank == 0 && now == 102 {
                        1
                    } else {
                        21
                    }
                );
                assert_eq!(
                    region.priority(),
                    if bank == 1 && now >= 101 {
                        129
                    } else {
                        125 + bank as i32
                    }
                );
                let size = if bank == 0 && now == 102 { 2 } else { 1 };
                assert_eq!(
                    region.bounds(),
                    Rect::from_xywh(if bank == 0 { 1 } else { 4 }, 1, size, size)
                );
                let mask = region.hit_mask().unwrap();
                assert!(mask.contains([0, 0], [0, 0]));
                assert!(!mask.contains([0, 0], [1, 0]));
            }
            assert_eq!(reads.borrow().len(), if now == 102 { 3 } else { 2 });
            send(&mut runtime, list, 298, 0, 0);
            for (x, color) in [
                (1, 0xffff_0000),
                (4, if now == 102 { 0 } else { 0xffff_0000 }),
            ] {
                assert_eq!(
                    runtime.drawing().target.as_ref().unwrap().pixel(x, 1),
                    Some(color)
                );
            }
        }
        let wrappers = handles.map(|id| scene.objects().get(id).unwrap().image_handle().unwrap());
        for (handle, task) in handles.into_iter().zip(tasks) {
            runtime
                .remove_fes_object(0, scene.objects_mut().unwrap(), handle)
                .unwrap();
            assert!(
                runtime
                    .input_state(1)
                    .unwrap()
                    .regions()
                    .get(task)
                    .is_none()
            );
            runtime.destroy(task).unwrap();
        }
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
        for wrapper in wrappers {
            assert!(runtime.images().get(wrapper).is_err());
        }
    }
}

#[test]
fn image_reload_preserves_wrapper_and_old_shares_while_updating_the_final_frame() {
    let (mut runtime, mut objects, list) = setup();
    let reads = Rc::new(RefCell::new(Vec::new()));
    runtime.set_resources(PixelFiles {
        files: BTreeMap::from([(b"sprite".to_vec(), pixel_file(0xff12_3456))]),
        reads: reads.clone(),
    });
    let group = define(&mut objects, "CIMAGE", "group");
    let sprite = define(&mut objects, "IMAGE", "sprite");
    let panel = define(&mut objects, "PLANE", "panel");
    initialize(
        &mut objects,
        "group",
        ObjectAttributes {
            position: [Some(1), Some(1), None],
            ..Default::default()
        },
    );
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            parent: Some(ObjectReference {
                name: "group".into(),
                bank: 0,
            }),
            file: Some(b"sprite".to_vec()),
            image_number: Some(7),
            priority: Some(5),
            display: Some(1),
            position: [Some(1), Some(0), None],
            ..Default::default()
        },
    );
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(0), Some(0), None],
            size: Some([8, 6]),
            vram: Some([0, 0]),
            priority: Some(500),
            display: Some(1),
            ..Default::default()
        },
    );
    for handle in [group, sprite, panel] {
        runtime.update_fes_object(0, &mut objects, handle).unwrap();
    }
    let wrapper = objects.get(sprite).unwrap().image_handle().unwrap();
    let old_pixels = runtime.images().pixel_handle(wrapper).unwrap();
    let shared = runtime.images_mut().share(wrapper).unwrap();
    let info = runtime.images().get(wrapper).unwrap().info();
    assert_eq!(info.offset, [0, 0]);
    assert_eq!(info.canvas_size, [1, 1]);
    assert_eq!(info.base, [3, 4]);
    assert_eq!(image(&objects, sprite).base_point, [2., 2.]);
    assert_eq!(image(&objects, sprite).size, [1, 1]);
    let rectangle = Rect::from_xywh(3, 3, 1, 1);
    assert_rectangle(&frame(&mut runtime, 1), rectangle, 0xff12_3456);

    let ObjectState::Image(state) = &mut objects.get_mut(sprite).unwrap().state else {
        panic!()
    };
    state.base.base_point = [3., 3.];
    objects.invalidate(sprite).unwrap();
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(2, 2, 1, 1),
        0xff12_3456,
    );
    assert_eq!(reads.borrow().len(), 1);

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            image_number: Some(999),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(objects.get(sprite).unwrap().image_handle(), Some(wrapper));
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert_eq!(image(&objects, sprite).base_point, [3., 3.]);
    assert!(!image(&objects, sprite).updates.needs_rebuild());
    assert_eq!(runtime.images().pixel_handle(shared).unwrap(), old_pixels);
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
    assert_rectangle(&frame(&mut runtime, 1), Rect::default(), 0);
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"sprite".to_vec()),
            image_number: Some(999),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().len(), 2);

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            image_number: Some(7),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_ne!(runtime.images().pixel_handle(wrapper).unwrap(), old_pixels);
    runtime
        .images_mut()
        .get_mut(shared)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 1, 1), 0xff77_8899);
    assert_rectangle(&frame(&mut runtime, 1), rectangle, 0xff12_3456);
    runtime.remove_fes_object(0, &mut objects, group).unwrap();
    assert!(runtime.images().pixel_handle(wrapper).is_err());
    assert_eq!(
        runtime.images().get(shared).unwrap().surface().pixel(0, 0),
        Some(0xff77_8899)
    );
    assert_rectangle(&frame(&mut runtime, 1), Rect::default(), 0);
    runtime.images_mut().release(shared).unwrap();
}

#[test]
fn image_files_share_the_mapped_frame_and_keep_the_first_successful_geometry() {
    let (mut runtime, mut objects, _) = setup();
    let reads = Rc::new(RefCell::new(Vec::new()));
    runtime.set_resources(PixelFiles {
        files: BTreeMap::from([
            (b"first".to_vec(), pixel_file(0)),
            (b"second".to_vec(), pixel_file(0xff44_5566)),
        ]),
        reads: reads.clone(),
    });
    let sprite = define(&mut objects, "IMAGE", "sprite");
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b" missing  first second missing ".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let wrapper = objects.get(sprite).unwrap().image_handle().unwrap();
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert_eq!(reads.borrow().len(), 4);
    let mut attributes = ObjectAttributes::default();
    attributes.frames[0] = Some(7);
    initialize(&mut objects, "sprite", attributes);
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().len(), 4);
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            image_number: Some(0),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let loaded = runtime.images().get(wrapper).unwrap();
    assert_eq!(loaded.surface().pixel(0, 0), Some(0x0044_5566));
    assert_eq!(loaded.info().base, [3, 4]);
    assert_eq!(image(&objects, sprite).base_point, [2., 2.]);
    assert_eq!(reads.borrow().len(), 8);
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(Vec::new()),
            size_resource: Some(b"unused:7".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().len(), 8);
    assert_ne!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"first\tsecond".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().last().unwrap(), b"first\tsecond");
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
}

struct DirectFiles(PixelFiles);

impl ResourceServices for DirectFiles {
    fn resolve(&mut self, _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
        panic!("直接路径不经过类别名称补全")
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.0.read(name, mode)
    }
}

#[test]
fn image_direct_paths_keep_spaces_bypass_categories_and_replace_requested_dimensions() {
    let (mut runtime, mut objects, _) = setup();
    let reads = Rc::new(RefCell::new(Vec::new()));
    runtime.set_resources(DirectFiles(PixelFiles {
        files: BTreeMap::from([(b"raw sprite.hg3".to_vec(), pixel_file(0xff12_3456))]),
        reads: reads.clone(),
    }));
    let sprite = define(&mut objects, "IMAGE", "sprite");
    let ObjectState::Image(state) = &mut objects.get_mut(sprite).unwrap().state else {
        panic!()
    };
    state.base.size = [123, 99];
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"raw sprite.hg3".to_vec()),
            image_number: Some(7),
            path_mode: Some(1),
            button_type: Some(1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let wrapper = objects.get(sprite).unwrap().image_handle().unwrap();
    let pixels = runtime.images().pixel_handle(wrapper).unwrap();
    assert_ne!(pixels, 0);
    assert_eq!(image(&objects, sprite).size, [1, 1]);
    assert_eq!(image(&objects, sprite).base_point, [3., 4.]);
    let info = runtime.images().get(wrapper).unwrap().info();
    assert_eq!(info.offset, [1, 2]);
    assert_eq!(info.canvas_size, [10, 20]);
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            path_mode: Some(-1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().len(), 1);

    let ObjectState::Image(state) = &mut objects.get_mut(sprite).unwrap().state else {
        panic!()
    };
    state.file.clear();
    state.base.base_point = [8., 9.];
    state.base.size = [9, 8];
    state.base.updates.request_rebuild();
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().len(), 1);
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), pixels);
    assert_eq!(image(&objects, sprite).base_point, [8., 9.]);
    assert_eq!(image(&objects, sprite).size, [9, 8]);
    assert!(!image(&objects, sprite).updates.needs_rebuild());

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"missing sprite.hg3".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert_eq!(image(&objects, sprite).base_point, [8., 9.]);
    assert_eq!(image(&objects, sprite).size, [9, 8]);
    assert_eq!(
        &*reads.borrow(),
        &[b"raw sprite.hg3".to_vec(), b"missing sprite.hg3".to_vec()]
    );
}

#[test]
fn image_raw_sources_keep_pixels_and_reappear_after_a_missing_ordinary_frame() {
    let (mut runtime, mut objects, list) = setup();
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut first = pixel_file(0xff11_2233);
    first[16..20].copy_from_slice(&9u32.to_le_bytes());
    let mut second = pixel_file(0xff44_5566);
    second[16..20].copy_from_slice(&2u32.to_le_bytes());
    let next = (first.len() - 12) as u32;
    first[12..16].copy_from_slice(&next.to_le_bytes());
    first.extend_from_slice(&second[12..]);
    runtime.set_resources(PixelFiles {
        files: BTreeMap::from([
            (b"raw sprite".to_vec(), first.clone()),
            (b"ordinary".to_vec(), pixel_file(0xff77_8899)),
            (b"other raw".to_vec(), pixel_file(0xffaa_bbcc)),
        ]),
        reads: reads.clone(),
    });
    let sprite = define(&mut objects, "IMAGE", "sprite");
    let panel = define(&mut objects, "PLANE", "panel");
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"raw sprite".to_vec()),
            image_number: Some(-1),
            path_mode: Some(3),
            priority: Some(5),
            display: Some(1),
            position: [Some(2), Some(1), None],
            ..Default::default()
        },
    );
    initialize(
        &mut objects,
        "panel",
        ObjectAttributes {
            position: [Some(0), Some(0), None],
            size: Some([8, 6]),
            priority: Some(500),
            display: Some(1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, panel).unwrap();
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let wrapper = objects.get(sprite).unwrap().image_handle().unwrap();
    let raw = objects.get(sprite).unwrap().raw_image_handle();
    assert_eq!(runtime.image_data(raw).unwrap(), first);
    assert_eq!(source_slot(&mut runtime, list, wrapper), Some(5));
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert_eq!(image(&objects, sprite).size, [-1; 2]);
    assert_eq!(image(&objects, sprite).base_point, [0.; 2]);
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(3, 3, 1, 1),
        0xff44_5566,
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(reads.borrow().as_slice(), &[b"raw sprite".to_vec()]);

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"ordinary".to_vec()),
            image_number: Some(7),
            path_mode: Some(0),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let pixels = runtime.images().pixel_handle(wrapper).unwrap();
    assert_ne!(pixels, 0);
    assert_eq!(objects.get(sprite).unwrap().raw_image_handle(), raw);
    assert_eq!(source_slot(&mut runtime, list, wrapper), Some(5));
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(3, 3, 1, 1),
        0xff77_8899,
    );
    let geometry = (
        image(&objects, sprite).size,
        image(&objects, sprite).base_point,
    );
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"other raw".to_vec()),
            image_number: Some(-1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let other = objects.get(sprite).unwrap().raw_image_handle();
    assert_ne!(other, raw);
    assert!(runtime.image_data(raw).is_err());
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), pixels);
    assert_eq!(
        (
            image(&objects, sprite).size,
            image(&objects, sprite).base_point
        ),
        geometry
    );
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(3, 3, 1, 1),
        0xff77_8899,
    );

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            image_number: Some(999),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(runtime.images().pixel_handle(wrapper).unwrap(), 0);
    assert_eq!(objects.get(sprite).unwrap().raw_image_handle(), other);
    assert_rectangle(
        &frame(&mut runtime, 1),
        Rect::from_xywh(4, 5, 1, 1),
        0xffaa_bbcc,
    );
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            image_number: Some(-1),
            file: Some(b"missing".to_vec()),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(objects.get(sprite).unwrap().raw_image_handle(), 0);
    assert!(runtime.image_data(other).is_err());
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
    assert_rectangle(&frame(&mut runtime, 1), Rect::default(), 0);

    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"raw sprite".to_vec()),
            priority: Some(-1),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    let raw = objects.get(sprite).unwrap().raw_image_handle();
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
    assert_eq!(source_slot(&mut runtime, list, wrapper), Some(65535));
    runtime.remove_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(source_slot(&mut runtime, list, wrapper), None);
    assert!(runtime.image_data(raw).is_err());
    assert!(runtime.images().pixel_handle(wrapper).is_err());
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);

    let sprite = define(&mut objects, "IMAGE", "unslotted");
    initialize(
        &mut objects,
        "unslotted",
        ObjectAttributes {
            file: Some(b"ordinary".to_vec()),
            image_number: Some(7),
            ..Default::default()
        },
    );
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(image(&objects, sprite).parent_priority, -1);
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
    runtime.remove_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
}

#[test]
fn fes_raw_data_is_released_with_its_owner_while_the_list_keeps_decoded_frames() {
    let (mut runtime, mut objects, list) = setup();
    runtime.set_resources(PixelFiles {
        files: BTreeMap::from([(b"raw".to_vec(), pixel_file(0xff12_3456))]),
        reads: Rc::default(),
    });
    let owner = runtime.create(0, kind::PLANE, b"owner", b"").unwrap();
    let child_list = runtime
        .create(owner, kind::IMAGE_LIST, b"list", b"")
        .unwrap();
    send(&mut runtime, child_list, 285, 8, 6);
    objects.set_image_list(child_list, Rect::from_xywh(0, 0, 8, 6));
    let sprite = define(&mut objects, "IMAGE", "sprite");
    initialize(
        &mut objects,
        "sprite",
        ObjectAttributes {
            file: Some(b"raw".to_vec()),
            image_number: Some(-1),
            priority: Some(5),
            display: Some(1),
            ..Default::default()
        },
    );
    runtime
        .update_fes_object(owner, &mut objects, sprite)
        .unwrap();
    let raw = objects.get(sprite).unwrap().raw_image_handle();
    let wrapper = objects.get(sprite).unwrap().image_handle().unwrap();
    objects.set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
    objects.invalidate(sprite).unwrap();
    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
    runtime.destroy(owner).unwrap();
    assert!(runtime.image_data(raw).is_err());
    assert!(runtime.images().pixel_handle(wrapper).is_err());
    send(&mut runtime, list, 298, 0, 0);
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(1, 2),
        Some(0xff12_3456)
    );
}

#[test]
fn list_animation_commands_share_random_and_automatic_updates_use_the_body_time() {
    for budget in [1, 4096] {
        let (mut runtime, mut objects, list) = setup();
        runtime.set_budget(budget, 10000);
        runtime.host_mut().random = katze_system2::kcs::ScriptRandom::seeded(17);
        let mut first = pixel_file(0xffff_0000);
        let mut second = pixel_file(0xff00_00ff);
        first[68..76].fill(0);
        second[68..76].fill(0);
        second[16..20].copy_from_slice(&2u32.to_le_bytes());
        let next = (first.len() - 12) as u32;
        first[12..16].copy_from_slice(&next.to_le_bytes());
        first.extend_from_slice(&second[12..]);
        let mut program = b"ANM\0".to_vec();
        program.resize(32, 0);
        let ops = [
            (1u32, [0i32, 0, 100]),
            (12, [128, 0, 0]),
            (0, [7, 3, 3]),
            (0, [2, 4, 4]),
        ];
        program[8..12].copy_from_slice(&(ops.len() as u32).to_le_bytes());
        for (opcode, values) in ops {
            program.extend(opcode.to_le_bytes());
            for value in values.into_iter().chain([0; 5]) {
                program.extend(0u32.to_le_bytes());
                program.extend(value.to_le_bytes());
            }
        }
        let reads = Rc::new(RefCell::new(Vec::new()));
        runtime.set_resources(PixelFiles {
            files: BTreeMap::from([
                (b"raw".to_vec(), first),
                (b"movement.anm".to_vec(), program),
                (b"invalid.anm".to_vec(), b"invalid".to_vec()),
            ]),
            reads: reads.clone(),
        });
        let sprite = define(&mut objects, "IMAGE", "sprite");
        initialize(
            &mut objects,
            "sprite",
            ObjectAttributes {
                file: Some(b"raw".to_vec()),
                image_number: Some(-1),
                priority: Some(5),
                position: [Some(2), Some(1), None],
                display: Some(1),
                ..Default::default()
            },
        );
        runtime.update_fes_object(0, &mut objects, sprite).unwrap();
        runtime.update(10, 10).unwrap();
        let mut name = b"movement.anm\0".to_vec();
        let load = |runtime: &mut TestRuntime, name: &mut [u8]| {
            runtime.dispatch_command_with_buffers(
                0,
                Command {
                    target: list,
                    sender: 0,
                    code: 299,
                    data: [5, 0, 0],
                },
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(name),
                },
            )
        };
        load(&mut runtime, &mut name).unwrap();
        assert_eq!(send(&mut runtime, list, 301, 5, 1), 0);
        let mut expected = katze_system2::kcs::ScriptRandom::seeded(17);
        expected.next_integer();
        assert_eq!(
            runtime.host().random.clone().next_integer(),
            expected.next_integer()
        );
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0x80ff_0000)
        );
        runtime.update(1000, 13).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0x8000_00ff)
        );
        load(&mut runtime, &mut b"missing.anm\0".to_vec()).unwrap();
        runtime.update(1001, 14).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0x8000_00ff)
        );
        send(&mut runtime, list, 301, 5, 0);
        let mut expected = runtime.host().random.clone();
        expected.next_integer();
        assert_eq!(send(&mut runtime, list, 306, 5, 0), 0);
        assert_eq!(
            runtime.host().random.clone().next_integer(),
            expected.next_integer()
        );
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0x80ff_0000)
        );
        runtime.update(1002, 100).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0x80ff_0000)
        );
        assert!(load(&mut runtime, &mut b"invalid.anm\0".to_vec()).is_err());
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
            Some(0xffff_0000)
        );
        assert_eq!(
            reads.borrow().as_slice(),
            [
                b"raw".to_vec(),
                b"movement.anm".to_vec(),
                b"missing.anm".to_vec(),
                b"invalid.anm".to_vec()
            ]
        );
    }
}

#[test]
fn fes_animation_events_defer_loading_and_consume_requests_before_failed_loads() {
    for budget in [1, 4096] {
        let (mut runtime, mut objects, list) = setup();
        runtime.set_budget(budget, 10000);
        let mut program = b"ANM\0".to_vec();
        program.resize(32, 0);
        program[8..12].copy_from_slice(&2u32.to_le_bytes());
        for (opcode, values) in [(12u32, [128i32, 0, 0]), (0, [7, 3, 3])] {
            program.extend(opcode.to_le_bytes());
            for value in values.into_iter().chain([0; 5]) {
                program.extend(0u32.to_le_bytes());
                program.extend(value.to_le_bytes());
            }
        }
        let reads = Rc::new(RefCell::new(Vec::new()));
        let mut pixels = pixel_file(0xffff_0000);
        pixels[68..76].fill(0);
        runtime.set_resources(PixelFiles {
            files: BTreeMap::from([
                (b"raw".to_vec(), pixels),
                (b"walk.anm".to_vec(), program),
                (b"invalid.anm".to_vec(), b"invalid".to_vec()),
            ]),
            reads: reads.clone(),
        });
        runtime
            .numbers_mut()
            .strings
            .insert(7, b"walk.custom".to_vec());
        let source = b"#DEFINE\nIMAGE sprite\n#OBJECT FILE IDN PRI POS DISP\nsprite raw -1 5 2,1 1\n#START\nsprite setanm $str7;\nsprite anm 0\nsprite anm stop\nsprite anm\nsprite setanm invalid\nexit 0\n";
        let mut interpreter =
            fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        let mut commands = 0;
        loop {
            match interpreter.run(runtime.numbers_mut(), budget).unwrap() {
                Step::Yield => (),
                Step::Event(fes::Event::CreateObject(definition)) => {
                    objects.define(&definition).unwrap();
                    objects.publish_bindings(&mut interpreter);
                    interpreter.resume();
                }
                Step::Event(fes::Event::InitializeObject(request)) => {
                    objects
                        .initialize(&request, |name| Ok(name.to_vec()))
                        .unwrap();
                    let sprite = objects.lookup("sprite", 0).unwrap();
                    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
                    runtime.update(10, 10).unwrap();
                    interpreter.resume();
                }
                Step::Event(fes::Event::Command(line)) => {
                    let before = reads.borrow().len();
                    assert!(
                        objects
                            .dispatch_image_animation(
                                &mut interpreter,
                                &line,
                                runtime.numbers_mut()
                            )
                            .unwrap()
                    );
                    assert_eq!(reads.borrow().len(), before);
                    let sprite = objects.lookup("sprite", 0).unwrap();
                    let request = objects
                        .get(sprite)
                        .unwrap()
                        .image_animation_request()
                        .unwrap();
                    assert!(request.is_pending());
                    assert_eq!(request.playing(), i32::from(matches!(commands, 1 | 3 | 4)));
                    assert_eq!(
                        request.name(),
                        if commands == 4 {
                            b"invalid.anm".as_slice()
                        } else {
                            b"walk.custom"
                        }
                    );
                    let result = runtime.update_fes_object(0, &mut objects, sprite);
                    assert_eq!(result.is_err(), commands == 4);
                    assert!(
                        !objects
                            .get(sprite)
                            .unwrap()
                            .image_animation_request()
                            .unwrap()
                            .is_pending()
                    );
                    runtime.update_fes_object(0, &mut objects, sprite).unwrap();
                    send(&mut runtime, list, 298, 0, 0);
                    assert_eq!(
                        runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
                        Some(if commands == 0 || commands == 4 {
                            0xffff_0000
                        } else {
                            0x80ff_0000
                        })
                    );
                    runtime.update(1000 + commands, 100 + commands).unwrap();
                    commands += 1;
                    interpreter.resume();
                }
                Step::Event(fes::Event::Exit(0)) => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(commands, 5);
        assert_eq!(
            reads.borrow().as_slice(),
            [
                b"raw".to_vec(),
                b"walk.anm".to_vec(),
                b"walk.anm".to_vec(),
                b"invalid.anm".to_vec()
            ]
        );
    }
}

#[test]
fn scene_object_commands_select_frames_reload_sizes_and_update_final_pixels() {
    for budget in [1, 4096] {
        let (mut runtime, _, list) = setup();
        runtime.set_budget(budget, 10000);
        runtime.numbers_mut().shared.resize(4, 0);
        let mut first = pixel_file(0xffff_0000);
        let mut second = pixel_file(0xff00_ff00);
        for pixels in [&mut first, &mut second] {
            pixels[48..56].fill(0);
            pixels[68..76].fill(0);
        }
        second[16..20].copy_from_slice(&8u32.to_le_bytes());
        let next = (first.len() - 12) as u32;
        first[12..16].copy_from_slice(&next.to_le_bytes());
        first.extend_from_slice(&second[12..]);
        let mut position = pixel_file(0);
        position[48..52].copy_from_slice(&3i32.to_le_bytes());
        position[52..56].copy_from_slice(&2i32.to_le_bytes());
        let reads = Rc::new(RefCell::new(Vec::new()));
        runtime.set_resources(PixelFiles {
            files: BTreeMap::from([
                (b"texture".to_vec(), first),
                (b"position".to_vec(), position),
            ]),
            reads: reads.clone(),
        });
        let program = fes::Program::from_text(b"#DEFINE\nIMAGE sprite\nPLANE panel\n#OBJECT FILE IDN PRI POS DISP\nsprite texture 7 5 1,1 0\n#OBJECT POS SIZE VRAM PRI DISP\npanel 0,0 8,6 0,0 500 1\n#START\n\\0=0\nsprite disp\nsprite pri 7\nwait 1\nsprite setid 8\nsprite getid 0\nsprite size (\\0+=1) (\\0+=1)\n$1=\\0\nwait 1\nsprite size 1 2\nwait 1\nsprite size -1 -1\nsprite pos2 position 7\nwait 1\nsprite disp 0\nwait 99\n").unwrap();
        let mut scene =
            katze_system2::runtime::fes::Scene::new(program, runtime.host().handle_allocator())
                .unwrap();
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
        for (now, color, point, read_count, size) in [
            (100, 0xffff_0000, [1, 1], 1, [1, 1]),
            (101, 0xff00_ff00, [1, 1], 2, [1, 2]),
            (102, 0xff00_ff00, [1, 1], 2, [1, 2]),
            (103, 0xff00_ff00, [3, 2], 4, [1, 1]),
            (104, 0, [3, 2], 4, [1, 1]),
        ] {
            let mut complete = false;
            for _ in 0..64 {
                if runtime.update_fes_scene(0, &mut scene, now).unwrap() {
                    complete = true;
                    break;
                }
            }
            assert!(complete);
            let sprite = scene.objects().lookup("sprite", 0).unwrap();
            let state = image(scene.objects(), sprite);
            assert_eq!(state.size, size);
            assert_eq!((state.priority, state.parent_priority), (27, 5));
            assert_eq!(reads.borrow().len(), read_count);
            if now > 100 {
                assert_eq!(&runtime.numbers().shared[..2], &[8, 2]);
            }
            send(&mut runtime, list, 298, 0, 0);
            assert_rectangle(
                &frame(&mut runtime, budget),
                Rect::from_xywh(point[0], point[1], 1, 1),
                color,
            );
        }
        assert_eq!(
            &*reads.borrow(),
            &[
                b"texture".to_vec(),
                b"texture".to_vec(),
                b"texture".to_vec(),
                b"position".to_vec()
            ]
        );
    }
}

#[test]
fn fes_scenes_run_image_commands_and_repeat_blocks_after_exit_notifications() {
    for budget in [1, 4096] {
        let (mut runtime, _, list) = setup();
        runtime.set_budget(budget, 10000);
        let mut pixels = pixel_file(0xffff_0000);
        pixels[68..76].fill(0);
        let mut animation = b"ANM\0".to_vec();
        animation.resize(32, 0);
        animation[8..12].copy_from_slice(&2u32.to_le_bytes());
        for (opcode, values) in [(12u32, [128i32, 0, 0]), (0, [7, 3, 3])] {
            animation.extend(opcode.to_le_bytes());
            for value in values.into_iter().chain([0; 5]) {
                animation.extend(0u32.to_le_bytes());
                animation.extend(value.to_le_bytes());
            }
        }
        let reads = Rc::new(RefCell::new(Vec::new()));
        runtime.set_resources(PixelFiles {
            files: BTreeMap::from([(b"raw".to_vec(), pixels), (b"walk.anm".to_vec(), animation)]),
            reads: reads.clone(),
        });
        let program = fes::Program::from_text(b"#DEFINE\nIMAGE sprite\n#OBJECT FILE IDN PRI POS DISP\nsprite raw -1 5 2,1 1\n#START\nsprite setanm walk\nsprite anm\ndecode\nsprite anm STOP\nwait 2\nsprite anm\nexit 7\n").unwrap();
        let mut scene =
            katze_system2::runtime::fes::Scene::new(program, runtime.host().handle_allocator())
                .unwrap();
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(list, Rect::from_xywh(0, 0, 8, 6));
        for now in [100, 101, 102, 1000] {
            let mut complete = false;
            for retry in 0..64 {
                if runtime
                    .update_fes_scene(0, &mut scene, now + if retry == 0 { 0 } else { 1000 })
                    .unwrap()
                {
                    complete = true;
                    break;
                }
            }
            assert!(complete);
            let sprite = scene.objects().lookup("sprite", 0).unwrap();
            let request = scene
                .objects()
                .get(sprite)
                .unwrap()
                .image_animation_request()
                .unwrap();
            assert_eq!(request.playing(), i32::from(now == 102));
            assert!(!request.is_pending());
            assert_eq!(scene.exit_code(), if now >= 102 { Some(7) } else { None });
            send(&mut runtime, list, 298, 0, 0);
            assert_eq!(
                runtime.drawing().target.as_ref().unwrap().pixel(3, 3),
                Some(0x80ff_0000)
            );
            assert_eq!(
                &reads.borrow()[..2],
                [b"raw".to_vec(), b"walk.anm".to_vec()]
            );
            assert_eq!(
                reads.borrow().len(),
                match now {
                    102 => 3,
                    1000 => 4,
                    _ => 2,
                }
            );
        }
    }
}
