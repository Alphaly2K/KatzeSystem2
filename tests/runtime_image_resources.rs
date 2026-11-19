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
    pixel_blob([0; 4])
}

fn pixel_blob(color: [u8; 4]) -> Vec<u8> {
    let compress = |bytes: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let delta = color.map(|value| (value << 1) ^ (0u8.wrapping_sub(value >> 7)));
    let planes = [6, 4, 2, 0].map(|shift| {
        delta.iter().enumerate().fold(0, |value, (i, byte)| {
            value | (((byte >> shift) & 3) << (i * 2))
        })
    });
    let data = compress(&planes);
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
        if name == b"anmtable.csv" {
            return Ok(b"image/anmtable.csv".to_vec());
        }
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
fn window_native_owns_a_composite_independent_of_its_source() {
    struct WindowFiles;
    impl ResourceServices for WindowFiles {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(match name {
                b"hg3" => Some(pixel_blob([0x11, 0x22, 0x33, 0xff])),
                b"hg2" => Some(katze_system2::hg2::encode_frame(
                    [1, 1],
                    &[0x44, 0x55, 0x66, 0xff],
                )?),
                b"other" => Some(b"unsupported".to_vec()),
                _ => None,
            })
        }
    }

    let mut code = Vec::new();
    for (i, address) in [64, 72, 80].into_iter().enumerate() {
        call(&mut code, 365, &[address]);
        ins(&mut code, 0x26, &[0]);
        handle_call(&mut code, 648, &[4, 3]);
        ins(&mut code, 0x26, &[4 + i as u32 * 4]);
        handle_call(&mut code, 366, &[]);
    }
    ins(&mut code, 0x3b, &[]);
    let mut rt = runtime(&code, 1);
    rt.set_resources(WindowFiles);
    let task = rt.create(0, kind::ORDINARY, b"window", b"").unwrap();
    let script = rt.object_mut(task).unwrap().interpreter_mut().unwrap();
    for (address, name) in [(64, b"hg3\0".as_slice()), (72, b"hg2\0"), (80, b"other\0")] {
        script.write(address, name).unwrap();
    }
    rt.update(0, 1).unwrap();
    let script = vm(&rt, task);
    let [hg3, hg2, empty] = [4, 8, 12].map(|address| word(script, address));
    assert!(script.stack_bytes().is_empty());
    assert!(rt.image_data(word(script, 0)).is_err());
    for (handle, x, y, pixel) in [(hg3, 1, 0, 0xff33_2211), (hg2, 0, 2, 0xff66_5544)] {
        let image = rt.images().get(handle).unwrap();
        assert_eq!(image.info().size, [4, 3]);
        assert_eq!(image.info().bit_depth, 32);
        assert_eq!(image.surface().pixel(x, y), Some(pixel));
        assert_eq!(image.surface().pixel(2, 1), Some(0));
    }
    assert_ne!(empty, 0);
    assert_eq!(rt.images().pixel_handle(empty).unwrap(), 0);
    rt.destroy(task).unwrap();
    for handle in [hg3, hg2, empty] {
        assert!(rt.images().pixel_handle(handle).is_err());
    }
}

#[test]
fn combined_bitmap_uses_frame_zero_and_survives_its_loading_task() {
    struct BitmapFiles;
    impl ResourceServices for BitmapFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            Ok([name, b".hg3"].concat())
        }

        fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            let mut bytes = blob();
            match name {
                b"art.hg3" => {}
                b"art_a.hg3" => bytes[16..20].fill(0),
                b"art_0b.hg3" => {
                    bytes[16..20].fill(0);
                    bytes[48..52].copy_from_slice(&1i32.to_le_bytes());
                    bytes[68..72].copy_from_slice(&4i32.to_le_bytes());
                }
                _ => return Ok(None),
            }
            Ok(Some(bytes))
        }
    }

    let mut code = Vec::new();
    call(&mut code, 460, &[64]);
    ins(&mut code, 0x26, &[0]);
    handle_call(&mut code, 377, &[]);
    ins(&mut code, 0x26, &[4]);
    for (address, output) in [(80, 8), (u32::MAX, 12)] {
        call(&mut code, 460, &[address]);
        ins(&mut code, 0x26, &[output]);
    }
    ins(&mut code, 0x3b, &[]);
    let mut rt = runtime(&code, 1);
    rt.set_resources(BitmapFiles);
    let task = rt.create(0, kind::ORDINARY, b"load", b"").unwrap();
    let script = rt.object_mut(task).unwrap().interpreter_mut().unwrap();
    script.write(64, b"art,a,b\0").unwrap();
    script.write(80, b"art\0").unwrap();
    rt.update(0, 1).unwrap();
    let script = vm(&rt, task);
    let handle = word(script, 0);
    assert_eq!(
        [word(script, 4), word(script, 8), word(script, 12)],
        [1, 0, 0]
    );
    assert!(script.stack_bytes().is_empty());
    rt.destroy(task).unwrap();
    let image = rt.images().get(handle).unwrap();
    assert_eq!(image.info().size, [4, 1]);
    assert_eq!(image.info().offset, [-2, 5]);
    assert_eq!(image.info().base, [3, 4]);
    assert_eq!(image.attribute(1), Some([9, 8, 7, 6, 5]));

    let mut release = Vec::new();
    call(&mut release, 114, &[handle]);
    ins(&mut release, 0x3b, &[]);
    rt.factory_mut().programs.0 = Program::from_code(&release, vec![], 128).unwrap();
    rt.create(0, kind::ORDINARY, b"release", b"").unwrap();
    rt.update(1, 1).unwrap();
    assert!(rt.images().get(handle).is_err());
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
                    b"anmtable.csv".to_vec(),
                    b"image/anmtable.csv".to_vec(),
                    b"image/\x89\xe6\x91\x9c.hg3".to_vec(),
                ]
            );
            runtime.destroy(task).unwrap();
            assert!(runtime.image_resource(handle).is_err());
        }
    }
}

#[test]
fn attribute_copies_merge_records_keep_shared_images_and_diagnose_freed_source_aliases() {
    struct AttributeFiles;
    impl ResourceServices for AttributeFiles {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"image");
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            let attributes = match name {
                b"target.hg3" => [(1, [99; 5]), (3, [11, 12, 13, 14, 15])],
                b"source.hg3" => [(1, [1, 2, 3, 4, 5]), (2, [6, 7, 8, 9, 10])],
                _ => return Ok(None),
            };
            let mut bytes = blob();
            bytes.truncate(bytes.len() - 36);
            if name == b"target.hg3" {
                bytes[48..52].copy_from_slice(&10u32.to_le_bytes());
            }
            for (index, (key, values)) in attributes.into_iter().enumerate() {
                bytes.extend(format!("ats{key:04}\0").bytes());
                bytes.extend(
                    [if index == 0 { 36 } else { 0 }, 20]
                        .into_iter()
                        .chain(values)
                        .flat_map(u32::to_le_bytes),
                );
            }
            Ok(Some(bytes))
        }
    }

    fn image(
        runtime: &Runtime<KcsFactory<Loader>>,
        handle: u32,
        resource: bool,
    ) -> &katze_system2::graphics::Image {
        if resource {
            runtime.image_resource(handle).unwrap()
        } else {
            runtime.images().get(handle).unwrap()
        }
    }

    for budget in [1, 4096] {
        for resource in [false, true] {
            let native = if resource { 725 } else { 537 };
            let load = if resource { 710 } else { 111 };
            let mut code = Vec::new();
            for (name, output) in [(64, 0), (80, 4)] {
                call(&mut code, load, &[name]);
                ins(&mut code, 0x26, &[output]);
            }
            call(&mut code, 65, &[]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            for (args, output) in [([0, u32::MAX], 8), ([u32::MAX, 0], 12)] {
                call(&mut code, native, &args);
                ins(&mut code, 0x26, &[output]);
            }
            for address in [0u32, 4] {
                code.extend([0x25, 0x13]);
                code.extend(address.to_le_bytes());
            }
            ins(&mut code, 0x2b, &[native, 8]);
            ins(&mut code, 0x26, &[16]);
            handle_call(
                &mut code,
                if resource { 724 } else { 536 },
                &[1, 20, 24, 28, 32, 36],
            );
            ins(&mut code, 0x26, &[40]);
            ins(&mut code, 0x26, &[44]);
            call(&mut code, 65, &[]);
            ins(&mut code, 0x25, &[0xabcdef01]);
            for _ in 0..2 {
                code.extend([0x25, 0x13]);
                code.extend(0u32.to_le_bytes());
            }
            ins(&mut code, 0x2b, &[native, 8]);
            let mut runtime = runtime(&code, budget);
            runtime.set_resources(AttributeFiles);
            let task = runtime
                .create(0, kind::ORDINARY, b"attributes", b"")
                .unwrap();
            let interpreter = runtime.object_mut(task).unwrap().interpreter_mut().unwrap();
            interpreter.write(64, b"target:7.hg3\0").unwrap();
            interpreter.write(80, b"source:7.hg3\0").unwrap();
            runtime.update(0, 1).unwrap();
            let target = word(vm(&runtime, task), 0);
            let source = word(vm(&runtime, task), 4);
            let before = image(&runtime, target, resource).clone();
            let source_before = image(&runtime, source, resource).clone();
            if resource {
                runtime.image_resource_updated(target, true).unwrap();
            } else {
                let shared = runtime.images_mut().share(target).unwrap();
                runtime
                    .object_mut(task)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(0, &shared.to_le_bytes())
                    .unwrap();
            }
            runtime.update(1, 1).unwrap();
            let interpreter = vm(&runtime, task);
            assert_eq!(
                [word(interpreter, 8), word(interpreter, 12)],
                [u32::from(resource); 2]
            );
            assert_eq!([word(interpreter, 16), word(interpreter, 40)], [1; 2]);
            assert_eq!(word(interpreter, 44), 0xdead_beef);
            assert!(interpreter.stack_bytes().is_empty());
            assert_eq!(
                interpreter.read(20, 20).unwrap(),
                [1u32, 2, 3, 4, 5]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            let output = image(&runtime, target, resource);
            assert_eq!(output.info(), before.info());
            assert_eq!(output.surface(), before.surface());
            assert_eq!(output.attribute(1), Some([1, 2, 3, 4, 5]));
            assert_eq!(output.attribute(2), Some([6, 7, 8, 9, 10]));
            assert_eq!(output.attribute(3), Some([11, 12, 13, 14, 15]));
            assert_eq!(image(&runtime, source, resource), &source_before);
            let copied = output.clone();
            if resource {
                assert!(!runtime.image_resource_updated(target, false).unwrap());
            }
            assert!(runtime.update(2, 1).unwrap_err().message.contains("已释放"));
            assert_eq!(image(&runtime, target, resource), &copied);
            assert_eq!(
                vm(&runtime, task).stack_bytes(),
                0xabcdef01u32.to_le_bytes()
            );
        }
    }
}

#[test]
fn failed_loads_skip_allocation_and_validate_name_sources() {
    let mut code = Vec::new();
    call(&mut code, 710, &[u32::MAX]);
    ins(&mut code, 0x26, &[120]);
    call(&mut code, 65, &[]);
    let names = [
        b"".as_slice(),
        b",,,",
        b"art,1",
        b"missing",
        b"empty",
        b"art:99",
        b"emptyanim",
        b"animate",
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
    assert!(
        requests
            .borrow()
            .iter()
            .any(|name| name == b"image/art_1.hg3")
    );

    {
        let (name, error) = (b"\xe7\x94\xbb\xe5\x83\x8f".as_slice(), "CP932");
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
fn snapshot_loading_uses_open_and_preserves_prefetched_image_precedence() {
    struct Sources(Rc<RefCell<usize>>);
    impl ResourceServices for Sources {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            if name == b"anmtable.csv" {
                return Ok(name.to_vec());
            }
            Ok([name, b".hg3"].concat())
        }
        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            if name == b"anmtable.csv" {
                return Ok(None);
            }
            assert_eq!(name, b"art.hg3");
            let mut bytes = blob();
            bytes[48..52].copy_from_slice(&13i32.to_le_bytes());
            Ok(Some(bytes))
        }
        fn open(
            &mut self,
            name: &[u8],
            _: ResourceMode,
        ) -> Result<Option<katze_system2::io::SharedSource>> {
            if name.ends_with(b".anm") {
                return Ok(None);
            }
            assert_eq!(name, b"art.hg3");
            *self.0.borrow_mut() += 1;
            Ok(Some(std::sync::Arc::new(blob())))
        }
    }
    for prefetched in [false, true] {
        let opens = Rc::new(RefCell::new(0));
        let mut sources = katze_system2::kcs::CachedResources::new(Sources(opens.clone()));
        if prefetched {
            let cache = sources.resource_cache().unwrap();
            cache.enqueue(b"art.hg3", 1).unwrap();
            cache.start();
            assert!(cache.advance(&mut sources).unwrap());
        }
        let mut code = Vec::new();
        call(&mut code, 710, &[64]);
        ins(&mut code, 0x26, &[0]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, 1);
        runtime.set_resources(sources);
        let task = runtime.create(0, kind::ORDINARY, b"snapshot", b"").unwrap();
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, b"art:7\0")
            .unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        assert_eq!(
            runtime.image_resource(handle).unwrap().info().offset[0],
            if prefetched { 13 } else { -2 }
        );
        assert_eq!(*opens.borrow(), usize::from(!prefetched));
    }
}

#[test]
fn animation_sources_start_with_the_smallest_cached_frame_for_single_and_layered_snapshots() {
    struct Animated {
        sidecar: Option<Vec<u8>>,
        image: Vec<u8>,
        alias: bool,
    }
    impl ResourceServices for Animated {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(if name == b"anmtable.csv" {
                name.to_vec()
            } else {
                [name, b".hg3"].concat()
            })
        }
        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(match name {
                b"single.anm" | b"art_1.anm" if !self.alias => self.sidecar.clone(),
                b"companion.anm" if self.alias => self.sidecar.clone(),
                b"anmtable.csv" if self.alias => {
                    Some(b"single\tcompanion\nart_1\tcompanion\n".to_vec())
                }
                b"single.hg3" | b"art_1.hg3" => Some(self.image.clone()),
                _ => None,
            })
        }
    }
    let mut image = blob();
    let frame_length = (image.len() - 12) as u32;
    image[12..16].copy_from_slice(&frame_length.to_le_bytes());
    image[16..20].copy_from_slice(&9u32.to_le_bytes());
    image[48..52].copy_from_slice(&9i32.to_le_bytes());
    let mut second = blob();
    second[16..20].copy_from_slice(&2u32.to_le_bytes());
    second[48..52].copy_from_slice(&2i32.to_le_bytes());
    image.extend(&second[12..]);
    let mut animation = b"ANM\0".to_vec();
    animation.extend([0u32, 1].into_iter().flat_map(u32::to_le_bytes));
    animation.resize(100, 0);
    animation[40..44].copy_from_slice(&9u32.to_le_bytes());
    for name in [b"single\0".as_slice(), b"art,1\0"] {
        for (sidecar, alias) in [
            (None, false),
            (Some(animation.clone()), false),
            (Some(vec![]), false),
            (Some(vec![1]), false),
            (Some(animation.clone()), true),
        ] {
            let loaded = sidecar.is_some();
            let mut code = Vec::new();
            call(&mut code, 710, &[64]);
            ins(&mut code, 0x26, &[0]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, 1);
            runtime.set_resources(Animated {
                sidecar,
                image: image.clone(),
                alias,
            });
            let task = runtime.create(0, kind::ORDINARY, b"snapshot", b"").unwrap();
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(64, name)
                .unwrap();
            runtime.update(0, 1).unwrap();
            let handle = word(vm(&runtime, task), 0);
            if loaded {
                assert_ne!(handle, 0);
                assert_eq!(runtime.image_resource(handle).unwrap().info().offset[0], 2);
                runtime.update(100, 1).unwrap();
                assert_eq!(runtime.image_resource(handle).unwrap().info().offset[0], 2);
                runtime.destroy(task).unwrap();
                assert!(runtime.image_resource(handle).is_err());
            } else {
                assert_eq!(handle, 0);
            }
        }
    }
}

#[test]
fn layered_resource_names_expand_each_component_and_scan_composite_alpha() {
    struct Layers(Requests);
    impl ResourceServices for Layers {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(if name == b"anmtable.csv" {
                name.to_vec()
            } else {
                [name, b".hg3"].concat()
            })
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            self.0.borrow_mut().push(name.to_vec());
            let x = match name {
                b"art_1.hg3" => -2i32,
                b"art_000a.hg3" => 3,
                _ => return Ok(None),
            };
            let mut bytes = blob();
            bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
            bytes[48..52].copy_from_slice(&x.to_le_bytes());
            Ok(Some(bytes))
        }
    }

    let mut code = Vec::new();
    call(&mut code, 710, &[64]);
    ins(&mut code, 0x26, &[0]);
    handle_call(&mut code, 721, &[]);
    ins(&mut code, 0x26, &[4]);
    call(&mut code, 65, &[]);
    for budget in [1, 4096] {
        let mut runtime = runtime(&code, budget);
        let requests = Requests::default();
        runtime.set_resources(Layers(requests.clone()));
        let task = runtime.create(0, kind::ORDINARY, b"layers", b"").unwrap();
        runtime
            .object_mut(task)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(64, b",art,,12,missing,0,alpha,\0")
            .unwrap();
        runtime.update(0, 1).unwrap();
        let handle = word(vm(&runtime, task), 0);
        assert_eq!(word(vm(&runtime, task), 4), 1);
        let image = runtime.image_resource(handle).unwrap();
        assert_eq!(image.info().size, [6, 1]);
        assert_eq!(image.info().offset, [-2, 5]);
        assert_eq!(image.info().base, [3, 4]);
        assert_eq!(image.attribute(1), Some([9, 8, 7, 6, 5]));
        assert_eq!(
            *requests.borrow(),
            [
                b"art_1.anm".to_vec(),
                b"anmtable.csv".to_vec(),
                b"art_1.hg3".to_vec(),
                b"art_0m.anm".to_vec(),
                b"art_0m.hg3".to_vec(),
                b"art_000a.anm".to_vec(),
                b"art_000a.hg3".to_vec(),
            ]
        );
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(handle).is_err());
    }
}

#[test]
fn layered_resource_changes_wait_for_drawing_and_rebuild_from_retained_sources() {
    use katze_system2::graphics::{ImageList, ImageListProperties};

    struct Layers;
    impl ResourceServices for Layers {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            let color = match name {
                b"layers.hg3" | b"layers_1.hg3" => [20, 40, 60, 64],
                b"layers_02.hg3" => [200, 180, 160, 128],
                _ => return Ok(None),
            };
            let mut bytes = pixel_blob(color);
            if name != b"layers.hg3" {
                bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
            }
            for (at, value) in [(48, 0u32), (52, 0), (56, 11), (60, 13)] {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            Ok(Some(bytes))
        }
    }

    for budget in [1, 4096] {
        for combined in [false, true] {
            let mut code = Vec::new();
            call(&mut code, 710, &[64]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            call(&mut code, 65, &[]);
            handle_call(&mut code, 729, &[50]);
            call(&mut code, 65, &[]);
            handle_call(&mut code, 736, &[0, 0, 1, 1, 0xff12_3456]);
            handle_call(&mut code, 729, &[100]);
            call(&mut code, 65, &[]);
            handle_call(&mut code, 737, &[3]);
            call(&mut code, 65, &[]);
            for (native, values) in [(718, [17, 19]), (719, [2, 3]), (720, [4, 5])] {
                handle_call(&mut code, native, &values);
                ins(&mut code, 0x2d, &[]);
            }
            handle_call(&mut code, 728, &[2]);
            handle_call(&mut code, 723, &[0]);
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            runtime.set_resources(Layers);
            let task = runtime.create(0, kind::ORDINARY, b"layers", b"").unwrap();
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(
                    64,
                    if combined {
                        b"layers,1,2.hg3\0"
                    } else {
                        b"layers:7.hg3\0"
                    },
                )
                .unwrap();
            runtime.update(0, 1).unwrap();
            let handle = word(vm(&runtime, task), 0);
            let mut list = ImageList::new(999, [1, 1], runtime.images_mut()).unwrap();
            list.set_resource(
                0,
                handle,
                ImageListProperties::default(),
                runtime.images_mut(),
            )
            .unwrap();
            let initial = if combined {
                [162, 151, 139, 161]
            } else {
                [20, 40, 60, 64]
            };
            assert_eq!(
                runtime.image_resource(handle).unwrap().surface().pixels(),
                initial
            );
            for (frame, before, after) in [
                (
                    1,
                    if combined { initial } else { [20, 40, 60, 32] },
                    if combined {
                        [149, 140, 131, 89]
                    } else {
                        [20, 40, 60, 32]
                    },
                ),
                (
                    2,
                    [86, 52, 18, 255],
                    if combined {
                        [149, 140, 131, 89]
                    } else {
                        [86, 52, 18, 255]
                    },
                ),
                (
                    3,
                    if combined {
                        [149, 140, 131, 89]
                    } else {
                        [169, 203, 237, 255]
                    },
                    if combined {
                        [102, 111, 120, 89]
                    } else {
                        [169, 203, 237, 255]
                    },
                ),
            ] {
                runtime.update(frame, 1).unwrap();
                assert!(runtime.image_resource_updated(handle, true).unwrap());
                assert_eq!(
                    runtime.image_resource(handle).unwrap().surface().pixels(),
                    before
                );
                assert!(
                    list.update_animations(frame, budget, runtime.images_mut())
                        .unwrap()
                );
                list.update(runtime.images_mut()).unwrap();
                assert_eq!(
                    runtime.image_resource(handle).unwrap().surface().pixels(),
                    after
                );
                assert!(!runtime.image_resource_updated(handle, false).unwrap());
                assert_eq!(
                    vm(&runtime, task).stack_bytes(),
                    0xdead_beefu32.to_le_bytes()
                );
            }
            runtime.update(4, 1).unwrap();
            let image = runtime.image_resource(handle).unwrap();
            assert_eq!(
                image.info().canvas_size,
                if combined { [11, 13] } else { [17, 19] }
            );
            assert_eq!(image.info().offset, if combined { [0; 2] } else { [2, 3] });
            assert_eq!(image.info().base, if combined { [3, 4] } else { [4, 5] });
            assert_eq!(image.alpha_mask(), if combined { 0 } else { 2 });
            runtime.destroy(task).unwrap();
            assert!(
                list.update_animations(5, budget, runtime.images_mut())
                    .unwrap()
            );
            list.update(runtime.images_mut()).unwrap();
            assert!(runtime.image_resource(handle).is_ok());
            list.release(runtime.images_mut()).unwrap();
            assert!(runtime.image_resource(handle).is_err());
        }
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
            (716, vec![0x80ff_8040]),
        ] {
            handle_call(&mut code, native, &args);
            call(&mut code, 65, &[]);
        }
        call(&mut code, 713, &[u32::MAX, 100, 100, 100]);
        call(&mut code, 737, &[u32::MAX, 0]);
        for native in [713, 716, 723, 727, 729, 737] {
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
        for frame in 1..=5 {
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
        assert_eq!(image.surface().pixels(), [64, 128, 127, 0]);
        assert_eq!(image.info().offset, [5, -2]);
        assert_eq!(image.info().base, [4, 3]);
        assert_eq!(image.attribute(1), Some([8, 9, 7, 6, 5]));
        runtime.update(6, 1).unwrap();
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(handle).is_ok());
        runtime.release_image_resource_reference(handle).unwrap();
        assert!(runtime.image_resource_updated(handle, false).is_err());
    }
    for (native, args) in [
        (713, vec![50, 50, 50]),
        (716, vec![u32::MAX]),
        (723, vec![0]),
        (726, vec![1, 1]),
        (727, vec![0]),
        (729, vec![100]),
        (730, vec![1, 1]),
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
fn resource_resize_updates_pixels_metadata_cache_and_native_stack_together() {
    use katze_system2::graphics::{ImageList, ImageListProperties, Rect};

    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 710, &[64]);
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x25, &[0xdead_beef]);
        handle_call(&mut code, 718, &[1, 1]);
        ins(&mut code, 0x2d, &[]);
        handle_call(&mut code, 736, &[0, 0, 1, 1, 0x801e_64c8]);
        call(&mut code, 65, &[]);
        for size in [[3, 2], [0, 2], [0, 2]] {
            handle_call(&mut code, 730, &size);
            call(&mut code, 65, &[]);
        }
        call(&mut code, 730, &[0]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        runtime.set_resources(Files(Requests::default()));
        let task = runtime.create(0, kind::ORDINARY, b"resize", b"").unwrap();
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
        runtime.update(1, 1).unwrap();
        let image = runtime.image_resource(handle).unwrap();
        assert_eq!(image.info().size, [3, 2]);
        assert_eq!(image.info().canvas_size, [3, 2]);
        assert_eq!(image.info().offset, [0, 10]);
        assert_eq!(image.info().base, [9, 8]);
        assert_eq!(image.attribute(1), Some([27, 16, 21, 12, 5]));
        assert_eq!(image.surface().pixels(), [200, 100, 30, 128].repeat(6));
        assert!(runtime.image_resource_updated(handle, true).unwrap());
        let mut list = ImageList::new(999, [32; 2], runtime.images_mut()).unwrap();
        list.update(runtime.images_mut()).unwrap();
        list.set_resource(
            0,
            handle,
            ImageListProperties {
                position: [16.; 2],
                base_point: [9., 8.],
                ..Default::default()
            },
            runtime.images_mut(),
        )
        .unwrap();
        assert_eq!(
            list.dirty_rectangles().collect::<Vec<_>>(),
            [Rect::from_xywh(7, 18, 3, 2)]
        );
        list.release(runtime.images_mut()).unwrap();
        runtime.update(2, 1).unwrap();
        assert!(runtime.image_resource_updated(handle, true).unwrap());
        let image = runtime.image_resource(handle).unwrap().clone();
        assert_eq!(image.info().size, [0, 2]);
        assert!(image.surface().pixels().is_empty());
        runtime.update(3, 1).unwrap();
        assert_eq!(runtime.image_resource(handle).unwrap(), &image);
        assert!(runtime.image_resource_updated(handle, true).unwrap());
        runtime.update(4, 1).unwrap();
        assert!(!runtime.image_resource_updated(handle, true).unwrap());
        assert_eq!(
            vm(&runtime, task).stack_bytes(),
            0xdead_beefu32.to_le_bytes()
        );
    }
}

#[test]
fn resource_and_wrapper_blending_keep_offsets_masks_and_native_protocol() {
    struct Pixel;
    impl ResourceServices for Pixel {
        fn resolve(&mut self, _: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            Ok(name.to_vec())
        }
        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(name
                .ends_with(b".hg3")
                .then(|| pixel_blob([80, 96, 112, 128])))
        }
    }
    fn transfer(code: &mut Vec<u8>, native: u32) {
        for (at, rectangle) in [
            (
                if native == 734 { 0u32 } else { 4 },
                [3, (-4i32) as u32, 0, u32::MAX],
            ),
            (if native == 734 { 4 } else { 0 }, [0, 0, 1, 1]),
        ] {
            code.extend([0x25, 0x13]);
            code.extend(at.to_le_bytes());
            for value in rectangle {
                ins(code, 0x25, &[value]);
            }
        }
        ins(code, 0x2b, &[native, 40]);
    }

    for native in [115, 734] {
        for budget in [1, 4096] {
            let resource = native == 734;
            let mut code = Vec::new();
            call(&mut code, if resource { 710 } else { 111 }, &[64]);
            ins(&mut code, 0x26, &[0]);
            call(&mut code, if resource { 733 } else { 112 }, &[4, 3]);
            ins(&mut code, 0x26, &[4]);
            code.extend([0x25, 0x13, 4, 0, 0, 0]);
            for value in [0, 0, 4, 3, 0xc010_2030] {
                ins(&mut code, 0x25, &[value]);
            }
            ins(&mut code, 0x2b, &[if resource { 736 } else { 117 }, 24]);
            if resource {
                code.extend([0x25, 0x13, 4, 0, 0, 0]);
                for value in [(-7i32) as u32, 11] {
                    ins(&mut code, 0x25, &[value]);
                }
                ins(&mut code, 0x2b, &[719, 12]);
                ins(&mut code, 0x2d, &[]);
            }
            ins(&mut code, 0x25, &[0xdead_beef]);
            call(&mut code, 65, &[]);
            transfer(&mut code, native);
            call(&mut code, 65, &[]);
            handle_call(&mut code, if resource { 728 } else { 564 }, &[1]);
            transfer(&mut code, native);
            if resource {
                handle_call(
                    &mut code,
                    native,
                    &[3, (-4i32) as u32, 1, 1, 0, (-2i32) as u32, 5, 1, 1],
                );
                handle_call(
                    &mut code,
                    native,
                    &[u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0],
                );
                call(&mut code, native, &[0; 6]);
            }
            call(&mut code, 65, &[]);
            let mut runtime = runtime(&code, budget);
            runtime.set_resources(Pixel);
            runtime.drawing_mut().enabled = true;
            runtime.drawing_mut().active = true;
            runtime.drawing_mut().target = Some(
                RenderTarget::from_tiles(
                    [4, 3],
                    [1, 1],
                    vec![Some(
                        Surface::from_bgra([4, 3], [1, 2, 3, 4].repeat(12)).unwrap(),
                    )],
                )
                .unwrap(),
            );
            let task = runtime.create(0, kind::ORDINARY, b"blend", b"").unwrap();
            runtime
                .object_mut(task)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(64, b"pixel:7.hg3\0")
                .unwrap();
            runtime.update(0, 1).unwrap();
            let source = word(vm(&runtime, task), 0);
            let target = word(vm(&runtime, task), 4);
            let image = |runtime: &Runtime<KcsFactory<Loader>>, handle| {
                if resource {
                    runtime.image_resource(handle)
                } else {
                    runtime.images().get(handle)
                }
                .unwrap()
                .clone()
            };
            let before = image(&runtime, target);
            let source_before = image(&runtime, source);
            if resource {
                for handle in [source, target] {
                    runtime.image_resource_updated(handle, true).unwrap();
                }
            }
            for (frame, pixel) in [(1, 0xe146_4441u32), (2, 0x4746_4441)] {
                runtime.update(frame, 1).unwrap();
                let after = image(&runtime, target);
                let mut expected = 0xc010_2030u32.to_le_bytes().repeat(12);
                expected[20..24].copy_from_slice(&pixel.to_le_bytes());
                assert_eq!(after.surface().pixels(), expected);
                assert_eq!(after.info(), before.info());
                assert_eq!(image(&runtime, source).surface(), source_before.surface());
                if resource {
                    for handle in [source, target] {
                        assert!(!runtime.image_resource_updated(handle, false).unwrap());
                    }
                }
                assert_eq!(
                    vm(&runtime, task).stack_bytes(),
                    0xdead_beefu32.to_le_bytes()
                );
                assert_eq!(
                    runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
                    Some(0x0403_0201)
                );
            }
        }
    }
}

#[test]
fn resource_copy_preserves_transparent_bgra_direction_and_update_flags() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        for (at, size, color) in [(0u32, [3, 2], 0x0011_2233), (4, [4, 3], 0xff44_5566)] {
            call(&mut code, 733, &size);
            ins(&mut code, 0x26, &[at]);
            code.extend([0x25, 0x13]);
            code.extend(at.to_le_bytes());
            for value in [0, 0, size[0], size[1], color] {
                ins(&mut code, 0x25, &[value]);
            }
            ins(&mut code, 0x2b, &[736, 24]);
        }
        ins(&mut code, 0x25, &[0xdead_beef]);
        call(&mut code, 65, &[]);
        for at in [0u32, 4] {
            code.extend([0x25, 0x13]);
            code.extend(at.to_le_bytes());
            for value in [0, 0, 3, 2] {
                ins(&mut code, 0x25, &[value]);
            }
        }
        ins(&mut code, 0x2b, &[735, 40]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 735, &[u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0]);
        call(&mut code, 735, &[0; 6]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        let task = runtime.create(0, kind::ORDINARY, b"copy", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let source = word(vm(&runtime, task), 0);
        let target = word(vm(&runtime, task), 4);
        runtime.image_resource_updated(source, true).unwrap();
        runtime.image_resource_updated(target, true).unwrap();
        runtime.update(1, 1).unwrap();
        assert_eq!(
            runtime.image_resource(source).unwrap().surface().pixels(),
            [0x33, 0x22, 0x11, 0].repeat(6)
        );
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(
                    runtime
                        .image_resource(target)
                        .unwrap()
                        .surface()
                        .pixel(x, y),
                    Some(if x < 3 && y >= 1 {
                        0x0011_2233
                    } else {
                        0xff44_5566
                    })
                );
            }
        }
        runtime.update(2, 1).unwrap();
        for handle in [source, target] {
            assert!(!runtime.image_resource_updated(handle, false).unwrap());
        }
        assert_eq!(
            vm(&runtime, task).stack_bytes(),
            0xdead_beefu32.to_le_bytes()
        );
    }
    for native in [734, 735] {
        let mut code = Vec::new();
        call(&mut code, 733, &[1, 1]);
        ins(&mut code, 0x26, &[0]);
        for value in [0, 0, 0, 1, 1] {
            ins(&mut code, 0x25, &[value]);
        }
        code.extend([0x25, 0x13]);
        code.extend(0u32.to_le_bytes());
        for value in [0, 0, 1, 1] {
            ins(&mut code, 0x25, &[value]);
        }
        ins(&mut code, 0x2b, &[native, 40]);
        let mut runtime = runtime(&code, 1);
        runtime.create(0, kind::ORDINARY, b"null", b"").unwrap();
        assert!(runtime.update(0, 1).unwrap_err().message.contains("零来源"));
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
fn resource_mask_controls_list_alpha_and_can_return_to_color_compositing() {
    use katze_system2::graphics::{ImageList, ImageListProperties};

    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 733, &[2, 1]);
        ins(&mut code, 0x26, &[0]);
        handle_call(&mut code, 736, &[0, 0, 1, 1, 0xff80_8080]);
        handle_call(&mut code, 736, &[1, 0, 1, 1, 0x0080_8080]);
        handle_call(&mut code, 728, &[4]);
        call(&mut code, 65, &[]);
        handle_call(&mut code, 728, &[0]);
        call(&mut code, 65, &[]);
        let mut runtime = runtime(&code, budget);
        let task = runtime.create(0, kind::ORDINARY, b"mask", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let resource = word(vm(&runtime, task), 0);

        let images = runtime.images_mut();
        let background = images.create(99, [2, 1]).unwrap();
        images
            .get_mut(background)
            .unwrap()
            .pixels_mut()
            .copy_from_slice(&[0xff12_3456u32, 0xffab_cdef].map(u32::to_le_bytes).concat());
        let mut list = ImageList::new(99, [2, 1], images).unwrap();
        list.set(0, background, ImageListProperties::default(), images)
            .unwrap();
        list.set_resource(1, resource, ImageListProperties::default(), images)
            .unwrap();
        list.redraw(images).unwrap();
        assert_eq!(
            images.get(list.image()).unwrap().surface().pixels(),
            [0xff12_3456u32, 0x00ab_cdef].map(u32::to_le_bytes).concat()
        );

        runtime.update(1, 1).unwrap();
        let images = runtime.images_mut();
        list.redraw(images).unwrap();
        assert_eq!(
            images.get(list.image()).unwrap().surface().pixels(),
            [0xff80_8080u32, 0xffab_cdef].map(u32::to_le_bytes).concat()
        );
    }
}

#[test]
fn resource_controls_preserve_alpha_cache_geometry_pixel_origin_and_return_stack() {
    for budget in [1, 4096] {
        for width in [0, 2] {
            let mut code = Vec::new();
            call(&mut code, 733, &[width, 2]);
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x25, &[0xdead_beef]);
            handle_call(&mut code, 877, &[]);
            ins(&mut code, 0x26, &[84]);
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
            handle_call(&mut code, 877, &[]);
            ins(&mut code, 0x26, &[88]);
            call(&mut code, 877, &[0]);
            ins(&mut code, 0x26, &[92]);
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
            assert_eq!(word(vm, 84), 0);
            assert_eq!(word(vm, 88), 0xfedc_ba98);
            assert_eq!(word(vm, 92), 0);
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
            assert_eq!(image.alpha_mask(), 0xfedc_ba98);
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
