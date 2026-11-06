use katze_system2::{
    Result,
    expression::State,
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
