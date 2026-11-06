use katze_system2::{
    Result,
    expression::State,
    graphics::{RenderTarget, Surface},
    kcs::{Interpreter, Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};
use std::{cell::RefCell, io::Write, rc::Rc};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        Ok(Some(if name == b"empty" {
            Program::from_code(&[0x3b], vec![0; 128], 128)?
        } else {
            self.0.clone()
        }))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for &value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, native: u32, args: &[u32]) {
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, args.len() as u32 * 4]);
}

fn handle_call(code: &mut Vec<u8>, native: u32, args: &[u32]) {
    code.extend([0x25, 0x13]);
    code.extend(0u32.to_le_bytes());
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, (args.len() as u32 + 1) * 4]);
}

fn runtime(code: &[u8], budget: usize) -> Runtime<KcsFactory<Loader>> {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(code, vec![0; 128], 128).unwrap())),
    );
    runtime.set_budget(budget, 1 << 20);
    runtime
}

fn vm(runtime: &Runtime<KcsFactory<Loader>>, task: u32) -> &Interpreter {
    runtime.object(task).unwrap().interpreter().unwrap()
}

fn word(vm: &Interpreter, at: u32) -> u32 {
    u32::from_le_bytes(vm.read(at, 4).unwrap().try_into().unwrap())
}

#[test]
fn resource_queries_preserve_native_stack_protocol_and_overlapping_outputs() {
    for budget in [1, 4096] {
        for width in [0, 2] {
            let mut code = Vec::new();
            call(&mut code, 733, &[width, 3]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            handle_call(&mut code, 712, &[17, 18]);
            handle_call(&mut code, 714, &[0x8000_0001, 0x8000_0002]);
            handle_call(&mut code, 715, &[24, 28]);
            handle_call(&mut code, 717, &[32, 36]);
            ins(&mut code, 0x26, &[4]);
            handle_call(
                &mut code,
                724,
                &[17, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX],
            );
            ins(&mut code, 0x26, &[8]);
            for native in [711, 712, 714, 715] {
                call(&mut code, native, &[0]);
            }
            for (native, at) in [(717, 12), (724, 40)] {
                call(&mut code, native, &[0]);
                ins(&mut code, 0x26, &[at]);
            }
            ins(&mut code, 0x26, &[44]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            let task = runtime.create(0, kind::ORDINARY, b"queries", b"").unwrap();
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .replace_external(vec![0xa5; 16])
                .unwrap();
            runtime.update(0, 1).unwrap();
            let vm = vm(&runtime, task);
            assert_eq!(vm.read(17, 5).unwrap(), [width as u8, 3, 0, 0, 0]);
            assert_eq!(
                vm.read(0x8000_0000, 7).unwrap(),
                [0xa5, width as u8, 3, 0, 0, 0, 0xa5]
            );
            assert_eq!(vm.read(24, 16).unwrap(), [0; 16]);
            for at in [4, 12, 40] {
                assert_eq!(word(vm, at), 1);
            }
            assert_eq!(word(vm, 8), 0);
            assert_eq!(word(vm, 44), 0xdead_beef);
            assert!(vm.stack_bytes().is_empty());
            let handle = word(vm, 0);
            assert_ne!(handle, 0);
            assert!(runtime.images().get(handle).is_err());
            let image = runtime.image_resource(handle).unwrap();
            assert_eq!(image.info().size, [width, 3]);
            assert_eq!(image.surface().pixels(), vec![0; width as usize * 3 * 4]);
        }
    }
}

#[test]
fn resources_release_owner_registration_and_keep_independent_references() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 733, &[2, 3]);
        ins(&mut code, 0x26, &[0]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 711, &[]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        for release_native in [false, true] {
            let task = runtime.create(0, kind::ORDINARY, b"create", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let handle = word(vm(&runtime, task), 0);
            runtime.retain_image_resource(handle).unwrap();
            if release_native {
                runtime.update(1, 1).unwrap();
            }
            runtime.destroy(task).unwrap();
            assert!(runtime.image_resource(handle).is_ok());
            runtime.release_image_resource_reference(handle).unwrap();
            assert!(runtime.image_resource(handle).is_err());
            runtime.update(2, 1).unwrap();
        }
        let task = runtime.create(0, kind::ORDINARY, b"reload", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    target: task,
                    sender: 0,
                    code: 161,
                    data: [0, 1, 0],
                },
                &mut CommandBuffers {
                    data1: Some(&mut b"empty".to_vec()),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        assert!(runtime.image_resource(handle).is_err());
        runtime.destroy(task).unwrap();
        runtime.update(1, 1).unwrap();
        let task = runtime.create(0, kind::ORDINARY, b"release", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(handle).is_err());
        runtime.release_image_resource_reference(0).unwrap();
    }
}

fn blob() -> Vec<u8> {
    let compress = |bytes: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let data = compress(&[0; 4]);
    let control = compress(&[9, 1]);
    let mut bytes = b"HG-3".to_vec();
    bytes.extend([12u32, 0x300, 0, 7].into_iter().flat_map(u32::to_le_bytes));
    bytes.extend(b"stdinfo\0");
    bytes.extend(
        [56u32, 40, 1, 1, 32, (-2i32) as u32, 5, 0, 20, 1, 3, 4]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    bytes.extend(b"img0000\0");
    bytes.extend(
        [
            (40 + data.len() + control.len()) as u32,
            (24 + data.len() + control.len()) as u32,
            0,
            1,
            data.len() as u32,
            4,
            control.len() as u32,
            2,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes),
    );
    bytes.extend(data);
    bytes.extend(control);
    bytes.extend(b"ats0001\0");
    bytes.extend(
        [0u32, 20, 9, 8, 7, 6, 5]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    bytes
}

type Requests = Rc<RefCell<Vec<Vec<u8>>>>;
struct Files(Requests);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        self.0.borrow_mut().push(name.to_vec());
        Ok([b"image/".as_slice(), name, b".hg3"].concat())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.0.borrow_mut().push(name.to_vec());
        Ok(match name {
            b"image/art.hg3" | b"image/\x89\xe6\x91\x9c.hg3" => Some(blob()),
            b"image/empty.hg3" | b"image/emptyanim.anm" => Some(Vec::new()),
            b"image/animate.anm" => Some(vec![1]),
            _ => None,
        })
    }
}

#[test]
fn loaded_resources_resolve_before_frame_selection_and_preserve_geometry_and_attributes() {
    for budget in [1, 4096] {
        for unicode in [0, 1] {
            let mut code = Vec::new();
            call(&mut code, 710, &[64]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            for (native, a, b) in [(712, 8, 12), (714, 16, 20), (715, 24, 28), (717, 32, 36)] {
                handle_call(&mut code, native, &[a, b]);
            }
            ins(&mut code, 0x26, &[4]);
            handle_call(&mut code, 724, &[0x10001, 40, 44, 48, 52, 56]);
            ins(&mut code, 0x26, &[60]);
            ins(&mut code, 0x26, &[120]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            let requests = Requests::default();
            runtime.set_resources(Files(requests.clone()));
            runtime.host_mut().unicode_text = unicode;
            let task = runtime.create(0, kind::ORDINARY, b"load", b"").unwrap();
            let name = if unicode != 0 {
                "画像:7\0".as_bytes()
            } else {
                b"\x89\xe6\x91\x9c:7\0"
            };
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(64, name)
                .unwrap();
            runtime.update(0, 1).unwrap();
            let vm = vm(&runtime, task);
            assert_eq!(word(vm, 4), 1);
            assert_eq!(
                vm.read(8, 52).unwrap(),
                [1u32, 1, 1, 20, (-2i32) as u32, 5, 3, 4, 9, 8, 7, 6, 5]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(word(vm, 60), 1);
            assert_eq!(word(vm, 120), 0xdead_beef);
            assert!(vm.stack_bytes().is_empty());
            let handle = word(vm, 0);
            assert_eq!(handle, 1);
            assert_eq!(runtime.image_resource(handle).unwrap().id(), 7);
            assert_eq!(
                runtime.image_resource(handle).unwrap().surface().pixels(),
                [0; 4]
            );
            assert_eq!(
                *requests.borrow(),
                [
                    b"\x89\xe6\x91\x9c:7".to_vec(),
                    b"image/\x89\xe6\x91\x9c:7.anm".to_vec(),
                    b"image/\x89\xe6\x91\x9c.hg3".to_vec(),
                ]
            );
            runtime.destroy(task).unwrap();
            assert!(runtime.image_resource(handle).is_err());
        }
    }
}

#[test]
fn failed_loads_do_not_allocate_and_animation_or_layer_sources_report_their_missing_protocols() {
    let mut code = Vec::new();
    call(&mut code, 710, &[u32::MAX]);
    ins(&mut code, 0x26, &[120]);
    call(&mut code, 65, &[]);
    let names = [
        b"".as_slice(),
        b",,,",
        b"missing",
        b"empty",
        b"art:99",
        b"emptyanim",
        b"art:7",
    ];
    for _ in names {
        call(&mut code, 710, &[64]);
        ins(&mut code, 0x26, &[120]);
        call(&mut code, 65, &[]);
    }
    let mut runtime = runtime(&code, 1);
    let requests = Requests::default();
    runtime.set_resources(Files(requests.clone()));
    let task = runtime.create(0, kind::ORDINARY, b"miss", b"").unwrap();
    runtime.update(0, 1).unwrap();
    assert_eq!(word(vm(&runtime, task), 120), 0);
    assert!(requests.borrow().is_empty());
    for (i, name) in names.iter().enumerate() {
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, &[name.to_vec(), vec![0]].concat())
            .unwrap();
        runtime.update(i as u32 + 1, 1).unwrap();
        assert_eq!(
            word(vm(&runtime, task), 120),
            u32::from(i == names.len() - 1)
        );
    }
    assert!(runtime.image_resource(1).is_ok());
    assert!(vm(&runtime, task).stack_bytes().is_empty());

    for (name, error) in [
        (b"art,1".as_slice(), "组合图层"),
        (b"animate", "ANM 来源"),
        (b"\xe7\x94\xbb\xe5\x83\x8f", "CP932"),
    ] {
        let mut code = Vec::new();
        call(&mut code, 710, &[64]);
        let mut runtime = self::runtime(&code, 1);
        runtime.set_resources(Files(Requests::default()));
        let task = runtime
            .create(0, kind::ORDINARY, b"unsupported", b"")
            .unwrap();
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, &[name.to_vec(), vec![0]].concat())
            .unwrap();
        assert!(runtime.update(0, 1).unwrap_err().message.contains(error));
        assert!(runtime.image_resource(1).is_err());
    }
}

#[test]
fn resource_transforms_keep_return_stack_and_shared_update_flag() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 710, &[64]);
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x25, &[0xdead_beef]);
        call(&mut code, 65, &[]);
        for (native, args) in [
            (737, vec![3]),
            (713, vec![50, 100, 200]),
            (723, vec![90]),
            (727, vec![1]),
        ] {
            handle_call(&mut code, native, &args);
            call(&mut code, 65, &[]);
        }
        call(&mut code, 713, &[u32::MAX, 100, 100, 100]);
        call(&mut code, 737, &[u32::MAX, 0]);
        for native in [713, 723, 727, 737] {
            call(&mut code, native, &[0]);
        }
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        runtime.set_resources(Files(Requests::default()));
        let task = runtime
            .create(0, kind::ORDINARY, b"transforms", b"")
            .unwrap();
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, b"art:7\0")
            .unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
        runtime.retain_image_resource(handle).unwrap();
        for frame in 1..=4 {
            runtime.update(frame, 1).unwrap();
            assert!(runtime.image_resource_updated(handle, false).unwrap());
            assert!(runtime.image_resource_updated(handle, true).unwrap());
            assert!(!runtime.image_resource_updated(handle, false).unwrap());
            assert_eq!(
                vm(&runtime, task).stack_bytes(),
                0xdead_beefu32.to_le_bytes()
            );
        }
        let image = runtime.image_resource(handle).unwrap();
        assert_eq!(image.surface().pixels(), [255, 255, 127, 0]);
        assert_eq!(image.info().offset, [5, -2]);
        assert_eq!(image.info().base, [4, 3]);
        assert_eq!(image.attribute(1), Some([8, 9, 7, 6, 5]));
        runtime.update(5, 1).unwrap();
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(handle).is_ok());
        runtime.release_image_resource_reference(handle).unwrap();
        assert!(runtime.image_resource_updated(handle, false).is_err());
    }
    for (native, args) in [
        (713, vec![50, 50, 50]),
        (723, vec![0]),
        (726, vec![1, 1]),
        (727, vec![0]),
    ] {
        let mut code = Vec::new();
        call(&mut code, 733, &[1, 1]);
        ins(&mut code, 0x26, &[0]);
        handle_call(&mut code, native, &args);
        let mut runtime = runtime(&code, 1);
        runtime.create(0, kind::ORDINARY, b"blank", b"").unwrap();
        assert!(
            runtime
                .update(0, 1)
                .unwrap_err()
                .message
                .contains("来源图层")
        );
    }
    let mut code = Vec::new();
    call(&mut code, 733, &[1, 1]);
    ins(&mut code, 0x26, &[0]);
    handle_call(&mut code, 737, &[3]);
    call(&mut code, 65, &[]);
    let mut runtime = runtime(&code, 1);
    let task = runtime
        .create(0, kind::ORDINARY, b"blank-filter", b"")
        .unwrap();
    runtime.update(0, 1).unwrap();
    let handle = word(vm(&runtime, task), 0);
    assert_eq!(
        runtime.image_resource(handle).unwrap().surface().pixels(),
        [255, 255, 255, 0]
    );
}

#[test]
fn resource_blur_keeps_two_expansion_stages_and_the_first_geometry_cache() {
    use katze_system2::graphics::{ImageList, ImageListProperties, Rect};

    struct BlurSource;
    impl ResourceServices for BlurSource {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            if name != b"blur.hg3" {
                return Ok(None);
            }
            let mut bytes = blob();
            for (field, value) in [(3, 3u32), (4, 2), (5, 7), (6, 5)] {
                bytes[36 + field * 4..40 + field * 4].copy_from_slice(&value.to_le_bytes());
            }
            Ok(Some(bytes))
        }
    }

    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 710, &[64]);
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x25, &[0xdead_beef]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 736, &[0, 0, 1, 1, 0xff1e_64c8]);
        handle_call(&mut code, 726, &[1, 1]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 726, &[0, 0]);
        call(&mut code, 65, &[]);
        call(&mut code, 726, &[0]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        runtime.set_resources(BlurSource);
        let task = runtime.create(0, kind::ORDINARY, b"blur", b"").unwrap();
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, b"blur:7.hg3\0")
            .unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
        for (frame, bounds) in [
            (1, Rect::from_xywh(9, 7, 3, 3)),
            (2, Rect::from_xywh(8, 6, 5, 5)),
        ] {
            runtime.update(frame, 1).unwrap();
            assert!(runtime.image_resource_updated(handle, true).unwrap());
            assert_eq!(
                vm(&runtime, task).stack_bytes(),
                0xdead_beefu32.to_le_bytes()
            );
            let image = runtime.image_resource(handle).unwrap();
            assert_eq!(image.info().size, [5; 2]);
            assert_eq!(image.info().offset, [1, 0]);
            assert_eq!(image.info().canvas_size, [7, 5]);
            assert_eq!(image.info().base, [4, 5]);
            assert_eq!(image.attribute(1), Some([9, 8, 7, 6, 5]));
            for y in 0..5 {
                for x in 0..5 {
                    let alpha = match (x, y) {
                        (2, 2) => 64,
                        (1 | 3, 2) | (2, 1 | 3) => 32,
                        (1 | 3, 1 | 3) => 16,
                        _ => 0,
                    };
                    assert_eq!(
                        image.surface().pixel(x, y),
                        Some(if alpha == 0 {
                            0
                        } else {
                            alpha << 24 | 0x1e_64c8
                        })
                    );
                }
            }
            let mut list = ImageList::new(999, [32; 2], runtime.images_mut()).unwrap();
            list.update(runtime.images_mut()).unwrap();
            list.set_resource(
                0,
                handle,
                ImageListProperties {
                    position: [10.; 2],
                    base_point: [3., 4.],
                    ..Default::default()
                },
                runtime.images_mut(),
            )
            .unwrap();
            assert_eq!(list.dirty_rectangles().collect::<Vec<_>>(), [bounds]);
            list.release(runtime.images_mut()).unwrap();
        }
        runtime.update(3, 1).unwrap();
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
    }
}

#[test]
fn difimage_references_hold_resources_copy_cached_base_and_query_current_attributes() {
    use katze_system2::{
        fes,
        runtime::fes::{ObjectState, Objects},
    };
    let mut code = Vec::new();
    call(&mut code, 710, &[64]);
    ins(&mut code, 0x26, &[0]);
    call(&mut code, 65, &[]);
    handle_call(&mut code, 720, &[9, 10]);
    ins(&mut code, 0x2d, &[]);
    call(&mut code, 65, &[]);
    handle_call(&mut code, 711, &[]);
    call(&mut code, 65, &[]);
    let mut runtime = runtime(&code, 1);
    runtime.set_resources(Files(Requests::default()));
    let task = runtime.create(0, kind::ORDINARY, b"source", b"").unwrap();
    runtime
        .object_mut(task)
        .unwrap()
        .interpreter_mut()
        .unwrap()
        .write(64, b"art:7\0")
        .unwrap();
    runtime.update(0, 1).unwrap();
    let resource = word(vm(&runtime, task), 0);
    let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
    runtime.numbers_mut().shared.resize(5, 0);
    let handles = objects
        .define(&fes::ObjectDefinition {
            kind: "DIFIMAGE",
            name: "item".into(),
            banks: 2,
        })
        .unwrap();
    let mut interpreter =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 999\n").unwrap()).unwrap();
    objects.publish_bindings(&mut interpreter);
    let line = |text: String| fes::Line {
        offset: 0,
        bytes: text.into_bytes(),
    };
    for round in 0..2 {
        if round == 1 {
            runtime.update(1, 1).unwrap();
        }
        runtime
            .dispatch_fes_reference(
                &mut objects,
                &mut interpreter,
                &line(format!("item refex {resource}")),
            )
            .unwrap();
        for &handle in &handles {
            let object = objects.get(handle).unwrap();
            let ObjectState::DifImage(state) = &object.state else {
                panic!()
            };
            assert_eq!(state.resource_handle(), Some(resource));
            assert_eq!(state.base.base_point, [3., 4.]);
            assert!(runtime.images().get(object.image_handle().unwrap()).is_ok());
        }
    }
    runtime
        .dispatch_fes_image_attributes(
            &objects,
            &mut interpreter,
            &line("item attr 1 0 1 2 3 4".into()),
        )
        .unwrap();
    assert_eq!(&runtime.numbers().shared[..5], &[9, 8, 7, 6, 5]);
    runtime.update(2, 1).unwrap();
    runtime.destroy(task).unwrap();
    assert!(runtime.image_resource(resource).is_ok());
    runtime
        .dispatch_fes_reference(&mut objects, &mut interpreter, &line("item refex".into()))
        .unwrap();
    assert!(runtime.image_resource(resource).is_err());
    for &handle in &handles {
        let object = objects.get(handle).unwrap();
        let ObjectState::DifImage(state) = &object.state else {
            panic!()
        };
        assert_eq!(state.resource_handle(), None);
        assert_eq!(object.image_handle(), None);
        assert_eq!(state.base.base_point, [3., 4.]);
    }
    runtime.release_fes_image_references(&mut objects).unwrap();
}

#[test]
fn resource_controls_preserve_alpha_cache_geometry_pixel_origin_and_return_stack() {
    for budget in [1, 4096] {
        for width in [0, 2] {
            let mut code = Vec::new();
            call(&mut code, 733, &[width, 2]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            for (operations, at) in [
                (vec![], 64),
                (vec![(736, vec![0, 0, width, 2, 0xff12_3456])], 68),
                (vec![(722, vec![0])], 72),
                (vec![(736, vec![0, 0, 1, 1, 0x8011_2233])], 76),
                (vec![(722, vec![u32::MAX])], 80),
            ] {
                for (native, args) in operations {
                    handle_call(&mut code, native, &args);
                }
                handle_call(&mut code, 721, &[]);
                ins(&mut code, 0x26, &[at]);
            }
            handle_call(&mut code, 728, &[0xfedc_ba98]);
            for (native, args) in [
                (718, vec![0, 19]),
                (719, vec![(-5i32) as u32, 6]),
                (720, vec![(-3i32) as u32, 4]),
            ] {
                handle_call(&mut code, native, &args);
                ins(&mut code, 0x26, &[4]);
            }
            for (native, a, b) in [(712, 8, 12), (714, 16, 20), (715, 24, 28), (717, 32, 36)] {
                handle_call(&mut code, native, &[a, b]);
            }
            ins(&mut code, 0x26, &[40]);
            for (native, at) in [(718, 44), (719, 48), (720, 52)] {
                call(&mut code, native, &[0]);
                ins(&mut code, 0x26, &[at]);
            }
            for native in [722, 728] {
                call(&mut code, native, &[0]);
            }
            call(&mut code, 736, &[0, 1, u32::MAX, 2, 3]);
            ins(&mut code, 0x26, &[56]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            runtime.drawing_mut().enabled = true;
            runtime.drawing_mut().active = true;
            runtime.drawing_mut().target = Some(
                RenderTarget::from_tiles(
                    [4, 4],
                    [1, 1],
                    vec![Some(
                        Surface::from_bgra([4, 4], 0xff44_5566u32.to_le_bytes().repeat(16))
                            .unwrap(),
                    )],
                )
                .unwrap(),
            );
            let task = runtime.create(0, kind::ORDINARY, b"controls", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let vm = vm(&runtime, task);
            assert_eq!(
                vm.read(64, 20).unwrap(),
                [u32::from(width != 0), u32::from(width != 0), 0, 0, 1]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                vm.read(8, 32).unwrap(),
                [width, 2, width, 19, (-5i32) as u32, 6, (-3i32) as u32, 4]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            for at in [4, 40, 44, 48, 52] {
                assert_eq!(word(vm, at), 1);
            }
            assert_eq!(word(vm, 56), 0xdead_beef);
            assert!(vm.stack_bytes().is_empty());
            let image = runtime.image_resource(word(vm, 0)).unwrap();
            assert_eq!(image.auxiliary_value(), 0xfedc_ba98);
            if width == 0 {
                assert!(image.surface().pixels().is_empty());
            } else {
                assert_eq!(
                    image.surface().pixels(),
                    [0xff12_3456u32, 0xff12_3456, 0x8011_2233, 0xff12_3456]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
            }
            let canvas = runtime.drawing().target.as_ref().unwrap();
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(
                        canvas.pixel(x, y),
                        Some(if (1..3).contains(&x) && y < 2 {
                            0
                        } else {
                            0xff44_5566
                        })
                    );
                }
            }
        }
    }
}

#[test]
fn zero_resource_alpha_query_reports_undefined_result_before_pushing_a_value() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0xdead_beef]);
    call(&mut code, 721, &[0]);
    let mut runtime = runtime(&code, 1);
    let task = runtime
        .create(0, kind::ORDINARY, b"undefined", b"")
        .unwrap();
    assert!(
        runtime
            .update(0, 1)
            .unwrap_err()
            .message
            .contains("未初始化")
    );
    assert_eq!(
        vm(&runtime, task).stack_bytes(),
        0xdead_beefu32.to_le_bytes()
    );
}
