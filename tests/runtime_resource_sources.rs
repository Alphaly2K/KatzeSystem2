use katze_system2::{
    Result,
    expression::State,
    graphics::{GraphicsFactory, ImageList, ImageListProperties, Rect, RenderTarget},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for value in args {
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
fn push_resource(code: &mut Vec<u8>) {
    code.extend([0x25, 0x13]);
    code.extend(0u32.to_le_bytes());
}
fn resource_call(code: &mut Vec<u8>, native: u32, args: &[u32]) {
    push_resource(code);
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, (args.len() as u32 + 1) * 4]);
}
fn register(code: &mut Vec<u8>) {
    ins(code, 0x25, &[1]);
    ins(code, 0x25, &[5]);
    push_resource(code);
    for value in [0, 0, 0, 0, 0, 1f32.to_bits(), 1f32.to_bits(), 300, 1, 0] {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[732, 52]);
}

#[test]
fn native_resource_registration_keeps_stack_clamps_opacity_and_releases_with_the_list() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 733, &[3, 2]);
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x25, &[0xdead_beef]);
        resource_call(&mut code, 736, &[0, 0, 3, 2, 0xff12_3456]);
        register(&mut code);
        call(&mut code, 65, &[]);
        call(&mut code, 65, &[]);
        resource_call(&mut code, 737, &[3]);
        call(&mut code, 65, &[]);
        call(&mut code, 65, &[]);
        register(&mut code);
        call(&mut code, 65, &[]);
        call(&mut code, 65, &[]);
        resource_call(&mut code, 711, &[]);
        call(&mut code, 65, &[]);
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Loader(
                    Program::from_code(&code, vec![0; 128], 128).unwrap(),
                )),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().surface_size = [3, 2];
        runtime.drawing_mut().target = Some(RenderTarget::new([3, 2]).unwrap());
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        assert_eq!(list, 1);
        runtime
            .dispatch_command(
                0,
                Command {
                    target: list,
                    sender: 0,
                    code: 285,
                    data: [3, 2, 0],
                },
            )
            .unwrap();
        let task = runtime.create(0, kind::ORDINARY, b"source", b"").unwrap();
        let mut output = [0; 4];
        runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    target: list,
                    sender: 0,
                    code: 291,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data1: Some(&mut output),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        let image = u32::from_le_bytes(output);
        runtime.update(0, 1).unwrap();
        let resource = u32::from_le_bytes(
            runtime
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap()
                .try_into()
                .unwrap(),
        );
        assert!(runtime.images().get(resource).is_err());
        assert!(runtime.image_resource(image).is_err());
        for (frame, expected) in [
            (1, 0xff12_3456),
            (2, 0xff12_3456),
            (3, 0xff12_3456),
            (4, 0xff12_3456),
            (5, 0xffed_cba9),
            (6, 0xffed_cba9),
        ] {
            runtime.update(frame, 1).unwrap();
            assert_eq!(
                runtime.images().get(image).unwrap().surface().pixel(0, 0),
                Some(expected),
                "frame={frame}"
            );
            assert_eq!(
                runtime
                    .object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .stack_bytes(),
                0xdead_beefu32.to_le_bytes()
            );
            if frame == 2 {
                assert!(runtime.image_resource_updated(resource, false).unwrap());
            }
            if frame == 3 {
                assert!(!runtime.image_resource_updated(resource, false).unwrap());
            }
        }
        runtime.destroy(task).unwrap();
        assert!(runtime.image_resource(resource).is_ok());
        runtime.destroy(list).unwrap();
        assert!(runtime.image_resource(resource).is_err());
    }
}

#[test]
fn hiding_and_removing_resources_use_cached_geometry_after_metadata_changes() {
    let mut code = Vec::new();
    call(&mut code, 733, &[2, 1]);
    ins(&mut code, 0x26, &[0]);
    call(&mut code, 65, &[]);
    resource_call(&mut code, 719, &[3, 0]);
    ins(&mut code, 0x2d, &[]);
    call(&mut code, 65, &[]);
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(&code, vec![0; 16], 128).unwrap())),
    );
    let task = runtime.create(0, kind::ORDINARY, b"source", b"").unwrap();
    runtime.update(0, 1).unwrap();
    let resource = u32::from_le_bytes(
        runtime
            .object(task)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(0, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    let mut list = ImageList::new(99, [8, 2], runtime.images_mut()).unwrap();
    list.set_resource(
        1,
        resource,
        ImageListProperties::default(),
        runtime.images_mut(),
    )
    .unwrap();
    list.update(runtime.images_mut()).unwrap();
    runtime.update(1, 1).unwrap();
    assert_eq!(
        runtime.image_resource(resource).unwrap().info().offset,
        [3, 0]
    );
    for visible in [false, true] {
        list.set_visible(1, visible, runtime.images()).unwrap();
        assert_eq!(
            list.dirty_rectangles().collect::<Vec<_>>(),
            [Rect::from_xywh(0, 0, 2, 1)]
        );
        list.update(runtime.images_mut()).unwrap();
    }
    list.remove(1, runtime.images_mut()).unwrap();
    assert_eq!(
        list.dirty_rectangles().collect::<Vec<_>>(),
        [Rect::from_xywh(0, 0, 2, 1)]
    );
    list.release(runtime.images_mut()).unwrap();
    runtime.destroy(task).unwrap();
    assert!(runtime.image_resource(resource).is_err());
}
