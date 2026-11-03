use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Rect, RenderTarget, Surface},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ObjectFactory, ProgramLoader, Runtime, TaskObject, kind},
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
fn background_copies_bottom_origin_crops_and_survives_source_release_clear_and_reinitialization() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Fallback },
    );
    runtime.host_mut().surface_size = [8, 6];
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut runtime, list, 285, 8, 6);
    let plane = runtime.tree().children(list).unwrap().next().unwrap();
    let source = runtime.images_mut().create(99, [4, 3]).unwrap();
    for (index, pixel) in runtime
        .images_mut()
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .chunks_exact_mut(4)
        .enumerate()
    {
        pixel.copy_from_slice(&(0xff00_0001 + index as u32).to_le_bytes());
    }
    let mut rectangle = [1i32, 1, 3, 3].map(i32::to_le_bytes).concat();
    runtime
        .dispatch_command_with_buffers(
            0,
            command(list, 302, 0, source),
            &mut CommandBuffers {
                data0: Some(&mut rectangle),
                data1: None,
            },
        )
        .unwrap();
    assert_eq!(runtime.drawables().get(plane).unwrap().display(), 1);
    let output = output_image(&mut runtime, list);
    runtime
        .images_mut()
        .get_mut(source)
        .unwrap()
        .pixels_mut()
        .fill(0);
    runtime.images_mut().release(source).unwrap();
    for reinitialize in [false, true] {
        if reinitialize {
            send(&mut runtime, list, 285, 8, 6);
            assert!(runtime.images().get(output).is_err());
            assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
        }
        let output = output_image(&mut runtime, list);
        send(&mut runtime, list, 297, 0, 0);
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
        assert!(
            runtime
                .images()
                .get(output)
                .unwrap()
                .surface()
                .pixels()
                .iter()
                .all(|&byte| byte == 0)
        );
        send(&mut runtime, list, 296, 0, 0);
        let surface = runtime.images().get(output).unwrap().surface();
        for (x, y, color) in [(0, 4, 2), (1, 4, 3), (0, 5, 6), (1, 5, 7)] {
            assert_eq!(surface.pixel(x, y), Some(0xff00_0000 | color));
        }
    }
    runtime
        .dispatch_command_with_buffers(
            0,
            command(list, 302, 1, 0),
            &mut CommandBuffers {
                data0: Some(&mut []),
                data1: None,
            },
        )
        .unwrap();
    assert_eq!(runtime.drawables().get(plane).unwrap().display(), 0);
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
    send(&mut runtime, list, 297, 0, 0);
    assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
            code.push(op);
            for n in args {
                code.push(3);
                code.extend(n.to_le_bytes());
            }
        }
        let mut code = Vec::new();
        ins(&mut code, 0x25, &[0xdead_beef]);
        for (native, args) in [
            (256, vec![1, 5, 3]),
            (260, vec![1, 2, (-1i32) as u32]),
            (262, vec![1, 0x8000_0001]),
            (263, vec![1, 7, 0]),
            (258, vec![1, 7]),
        ] {
            for n in &args {
                ins(&mut code, 0x25, &[*n]);
            }
            ins(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
        }
        if name == b"flush" {
            for target in [1, 0, 999] {
                ins(&mut code, 0x25, &[target]);
                ins(&mut code, 0x2b, &[342, 4]);
            }
        }
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![0; 4], 128)?))
    }
}

#[test]
fn ordinary_list_natives_initialize_the_plane_and_submit_at_the_integer_offset() {
    for (budget, force) in [(1, false), (1, true), (4096, false), (4096, true)] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().surface_size = [3, 2];
        let mut surface = Surface::new([5, 3]).unwrap();
        surface.clear(0xffab_cdef);
        let drawing = runtime.drawing_mut();
        drawing.enabled = true;
        drawing.active = true;
        drawing.target =
            Some(RenderTarget::from_tiles([5, 3], [1, 1], vec![Some(surface)]).unwrap());
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        assert_eq!(list, 1);
        let script = runtime
            .create(
                0,
                kind::ORDINARY,
                if force { b"flush" } else { b"setup" },
                b"",
            )
            .unwrap();
        runtime.update(0, 0).unwrap();
        let vm = runtime.object(script).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(0, 4).unwrap(), 0xdead_beefu32.to_le_bytes());
        assert!(vm.stack_bytes().is_empty());
        let plane = runtime.tree().children(list).unwrap().next().unwrap();
        assert_eq!(runtime.tree().node(plane).unwrap().kind, kind::PLANE);
        assert_eq!(runtime.drawables().get(plane).unwrap().display(), 0);
        let image = output_image(&mut runtime, list);
        assert_eq!(
            runtime.images().get(image).unwrap().surface().size(),
            [5, 3]
        );
        let mut x = [0; 4];
        let mut y = [0; 4];
        runtime
            .dispatch_command_with_buffers(
                0,
                command(list, 290, 0, 0),
                &mut CommandBuffers {
                    data0: Some(&mut x),
                    data1: Some(&mut y),
                },
            )
            .unwrap();
        assert_eq!((i32::from_le_bytes(x), i32::from_le_bytes(y)), (2, -1));
        assert_eq!(
            runtime.drawing_mut().target.as_ref().unwrap().pixel(2, 0),
            Some(if force { 0 } else { 0xffab_cdef })
        );
        runtime.update(1, 0).unwrap();
        let target = runtime.drawing_mut().target.as_ref().unwrap();
        assert_eq!(target.pixel(1, 0), Some(0xffab_cdef));
        assert_eq!(target.pixel(2, 0), Some(0));
        assert_eq!(target.pixel(4, 1), Some(0));
        assert_eq!(target.pixel(2, 2), Some(0xffab_cdef));
    }
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

#[test]
fn pixel_lookup_uses_signed_keys_and_preserves_missing_output_and_buffer_tails() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Fallback },
    );
    runtime.host_mut().surface_size = [2, 2];
    runtime.drawing_mut().target = Some(RenderTarget::new([2, 2]).unwrap());
    let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut runtime, list, 285, 2, 2);
    let source = runtime.images_mut().create(99, [1, 1]).unwrap();
    let pixels = runtime.images().pixel_handle(source).unwrap();
    let mut record: Vec<_> = [
        pixels,
        0,
        0,
        0,
        0,
        0,
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
    for slot in [7, 0x8002] {
        runtime
            .dispatch_command_with_buffers(
                0,
                command(list, 286, slot, 0),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut record),
                },
            )
            .unwrap();
    }
    runtime.images_mut().release(source).unwrap();
    for (query, expected) in [(0, None), (pixels, Some(0x8002)), (u32::MAX, None)] {
        let mut slot = [0xa5; 8];
        let found = runtime
            .dispatch_command_with_buffers(
                0,
                command(list, 307, query, 0),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut slot),
                },
            )
            .unwrap();
        assert_eq!(found, u32::from(expected.is_some()));
        assert_eq!(
            u32::from_le_bytes(slot[..4].try_into().unwrap()),
            expected.unwrap_or(0xa5a5a5a5)
        );
        assert_eq!(slot[4..], [0xa5; 4]);
    }
    send(&mut runtime, list, 287, 0x8002, 0);
    let mut slot = [0; 4];
    assert_eq!(
        runtime
            .dispatch_command_with_buffers(
                0,
                command(list, 307, pixels, 0),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut slot)
                }
            )
            .unwrap(),
        1
    );
    assert_eq!(u32::from_le_bytes(slot), 7);
    send(&mut runtime, list, 287, 7, 0);
    assert_eq!(send(&mut runtime, list, 307, pixels, 0), 0);
}
