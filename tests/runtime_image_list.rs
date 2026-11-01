use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Rect, RenderTarget, Surface},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

struct Fallback;
impl ObjectFactory for Fallback {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

fn command(id: u32, code: u32, a: u32, b: u32) -> Command {
    Command {
        target: id,
        sender: 0,
        code,
        data: [a, b, 0],
    }
}

fn send<F: ObjectFactory>(runtime: &mut Runtime<F>, id: u32, code: u32, a: u32, b: u32) -> u32 {
    runtime
        .dispatch_command(0, command(id, code, a, b))
        .unwrap()
}

fn output_image<F: ObjectFactory>(runtime: &mut Runtime<F>, id: u32) -> u32 {
    let mut buffer = [0xa5; 8];
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 291, 0, 0),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut buffer),
            },
        )
        .unwrap();
    assert_eq!(&buffer[4..], &[0xa5; 4]);
    u32::from_le_bytes(buffer[..4].try_into().unwrap())
}

#[test]
fn image_list_tasks_create_one_plane_update_shared_pixels_and_release_their_wrappers() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Fallback },
    );
    runtime.host_mut().surface_size = [5, 3];
    let drawing = runtime.drawing_mut();
    drawing.enabled = true;
    drawing.active = true;
    drawing.target = Some(
        RenderTarget::from_tiles([3, 2], [2, 2], vec![Some(Surface::new([3, 2]).unwrap()); 4])
            .unwrap(),
    );
    let id = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut runtime, id, 286, 0, 0);
    assert!(runtime.dispatch_command(0, command(id, 291, 0, 0)).is_err());
    assert_eq!(send(&mut runtime, id, 13, 0, 0), kind::IMAGE_LIST);
    send(&mut runtime, id, 285, 5, 3);
    let plane = runtime.tree().children(id).unwrap().next().unwrap();
    assert_eq!(runtime.tree().node(plane).unwrap().kind, kind::PLANE);
    let drawable = runtime.drawables().get(plane).unwrap();
    assert_eq!(drawable.display(), 0);
    assert_eq!(
        drawable.geometry_transform(false).unwrap().base_point(),
        [0., 0., 1000.]
    );
    let mut width = [0; 4];
    let mut height = [0; 4];
    runtime
        .dispatch_command_with_buffers(
            0,
            command(plane, 75, 0, 0),
            &mut CommandBuffers {
                data0: Some(&mut width),
                data1: Some(&mut height),
            },
        )
        .unwrap();
    assert_eq!(
        [f32::from_le_bytes(width), f32::from_le_bytes(height)],
        [5., 3.]
    );
    let source = runtime.images_mut().create(99, [2, 2]).unwrap();
    runtime
        .images_mut()
        .get_mut(source)
        .unwrap()
        .fill_rect_from_bottom(Rect::from_xywh(0, 0, 2, 2), 0xff12_3456);
    let pixels = runtime.images().pixel_handle(source).unwrap();
    let mut properties: Vec<u8> = [
        pixels,
        0,
        0,
        0,
        1f32.to_bits(),
        1f32.to_bits(),
        0,
        0,
        0,
        1f32.to_bits(),
        1f32.to_bits(),
        255,
        1,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    properties[..4].copy_from_slice(&source.to_le_bytes());
    assert!(
        runtime
            .dispatch_command_with_buffers(
                0,
                command(id, 286, 7, 0),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut properties)
                },
            )
            .is_err()
    );
    assert_eq!(send(&mut runtime, id, 294, 0, 0), 0);
    properties[..4].copy_from_slice(&pixels.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 286, 7, 0),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut properties),
            },
        )
        .unwrap();
    assert_eq!(send(&mut runtime, id, 294, 0, 0), 1);
    let mut priority = [0; 4];
    for expected in [7i32, -1] {
        assert_eq!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    command(id, 304, 7, 0),
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut priority),
                    },
                )
                .unwrap(),
            1
        );
        assert_eq!(i32::from_le_bytes(priority), expected);
        assert_eq!(send(&mut runtime, id, 305, 7, u32::MAX), 1);
    }
    assert_eq!(send(&mut runtime, id, 305, 99, 0), 0);
    assert_eq!(send(&mut runtime, id, 304, 99, 0), 0);
    runtime.images_mut().release(source).unwrap();
    let output = output_image(&mut runtime, id);
    runtime.update(0, 0).unwrap();
    assert_eq!(
        runtime.images().get(output).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0xff12_3456)
    );
    let mut clip = [2i32, 1, 3, 3].map(i32::to_le_bytes).concat();
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 295, 7, 0),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut clip),
            },
        )
        .unwrap();
    send(&mut runtime, id, 296, 0, 0);
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0)
    );
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(2, 1),
        Some(0xff12_3456)
    );
    send(&mut runtime, id, 295, 7, 0);
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 286, 7, 0),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut properties),
            },
        )
        .unwrap();
    send(&mut runtime, id, 296, 0, 0);
    let mut short = [0; 1];
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 295, 99, 1),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut short),
            },
        )
        .unwrap();
    send(&mut runtime, id, 310, 0, 0);
    send(&mut runtime, id, 293, 7, 0);
    runtime.update(1, 1).unwrap();
    assert_eq!(
        runtime.images().get(output).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    send(&mut runtime, id, 296, 0, 0);
    assert_eq!(
        runtime.images().get(output).unwrap().surface().pixel(1, 1),
        Some(0)
    );
    send(&mut runtime, id, 292, 0, 0);
    send(&mut runtime, id, 293, 7, 1);
    runtime.update(2, 1).unwrap();
    assert_eq!(
        runtime.images().get(output).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0)
    );
    send(&mut runtime, id, 298, 0, 0);
    assert_eq!(
        runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
        Some(0xff12_3456)
    );
    send(&mut runtime, id, 292, 0, 2);
    runtime.drawing_mut().enabled = false;
    runtime.update(3, 1).unwrap();
    runtime.drawing_mut().target.as_mut().unwrap().clear_dirty();
    runtime.drawing_mut().enabled = true;
    runtime.update(4, 1).unwrap();
    assert!(
        runtime
            .drawing()
            .target
            .as_ref()
            .unwrap()
            .dirty_tiles()
            .iter()
            .all(|&dirty| !dirty)
    );
    let alias = runtime.images_mut().share(output).unwrap();
    send(&mut runtime, id, 289, 4, 5);
    send(&mut runtime, id, 285, 2, 1);
    assert_eq!(
        runtime.tree().children(id).unwrap().collect::<Vec<_>>(),
        [plane]
    );
    assert!(runtime.images().get(output).is_err());
    assert_eq!(
        runtime.images().get(alias).unwrap().surface().pixel(1, 1),
        Some(0xff12_3456)
    );
    let replacement = output_image(&mut runtime, id);
    assert_ne!(replacement, output);
    let mut x = [0; 4];
    let mut y = [0; 4];
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 290, 0, 0),
            &mut CommandBuffers {
                data0: Some(&mut x),
                data1: Some(&mut y),
            },
        )
        .unwrap();
    assert_eq!([i32::from_le_bytes(x), i32::from_le_bytes(y)], [4, 5]);
    runtime.destroy(id).unwrap();
    assert!(runtime.object(plane).is_none());
    assert!(runtime.drawables().get(plane).is_none());
    assert!(runtime.images().get(replacement).is_err());
    assert!(runtime.images().get(alias).is_ok());
    runtime.images_mut().release(alias).unwrap();
}

#[test]
fn failed_child_creation_preserves_the_list_until_reinitialization_and_retries_the_plane() {
    struct FailPlaneOnce(bool);
    impl ObjectFactory for FailPlaneOnce {
        fn create(
            &mut self,
            id: u32,
            object_kind: u32,
            name: &[u8],
        ) -> Result<Box<dyn TaskObject>> {
            if object_kind == kind::PLANE && self.0 {
                self.0 = false;
                return Err(Error::new(0, "合成平面创建失败"));
            }
            GraphicsFactory { fallback: Fallback }.create(id, object_kind, name)
        }
    }
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        FailPlaneOnce(true),
    );
    runtime.host_mut().surface_size = [3, 2];
    runtime.drawing_mut().target = Some(RenderTarget::new([3, 2]).unwrap());
    let id = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    assert!(runtime.dispatch_command(0, command(id, 285, 3, 2)).is_err());
    let original = output_image(&mut runtime, id);
    assert!(runtime.tree().children(id).unwrap().next().is_none());
    send(&mut runtime, id, 285, 2, 1);
    assert_eq!(runtime.tree().children(id).unwrap().count(), 1);
    assert!(runtime.images().get(original).is_err());
    let replacement = output_image(&mut runtime, id);
    send(&mut runtime, id, 3, 0, 0);
    send(&mut runtime, id, 286, 0, 0);
    assert!(runtime.images().get(replacement).is_ok());
    runtime.update(0, 0).unwrap();
    assert!(runtime.images().get(replacement).is_err());
}
