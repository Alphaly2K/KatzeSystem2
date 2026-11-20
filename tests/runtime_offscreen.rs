use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, Projection, RenderTarget, TextureFilter, Transform},
    host::{DrawList, FrameCanvas, GraphicsBackend, SoftwareBackend, Surface},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

struct Source(Program);
impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
type Engine = Runtime<KcsFactory<Source>>;

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn program(args: &[u8]) -> Program {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[0xdeadbeef]);
    let mut stack = vec![0xa5; (4 - args.len() % 4) % 4];
    stack.extend(args);
    for word in stack.chunks_exact(4) {
        instruction(
            &mut code,
            0x25,
            &[u32::from_le_bytes(word.try_into().unwrap())],
        );
    }
    instruction(&mut code, 0x2b, &[568, args.len() as u32]);
    instruction(&mut code, 0x3b, &[]);
    Program::from_code(&code, vec![0x5a; 256], 128).unwrap()
}

fn engine(args: &[u8], view: bool) -> Engine {
    let mut rt = engine_program(program(args), view);
    rt.set_graphics_backend(SoftwareBackend);
    rt
}

fn engine_program(program: Program, view: bool) -> Engine {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Source(program)),
    );
    rt.host_mut().surface_size = [8, 4];
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().active = false;
    rt.drawing_mut().target = Some(RenderTarget::new([8, 4]).unwrap());
    if view {
        rt.set_frame_view(
            Projection {
                translation: [1.0, 0.0],
                ..Projection::default()
            },
            *Transform::default().matrix(),
            TextureFilter::Point,
        );
    }
    for (i, depth) in [9.0, 10.0, 20.0].into_iter().enumerate() {
        let id = 100 + i as u32;
        rt.drawables_mut()
            .insert(
                id,
                Drawable::rectangle(
                    [i as f32 * 2.0, 1.0, 0.0],
                    [1.0, 2.0],
                    0xff123456 + i as u32,
                ),
            )
            .unwrap();
        rt.drawables_mut().set_display(id, 1);
        rt.drawables_mut().set_base_point(id, [0.0, 0.0, depth]);
    }
    rt
}

fn call(rt: &mut Engine) -> u32 {
    let id = rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    rt.update(0, 0).unwrap();
    let vm = rt.object(id).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), 0xdeadbeefu32.to_le_bytes());
    assert_eq!(vm.read(0, 256).unwrap(), [0x5a; 256]);
    id
}

fn execute(rt: &mut Engine, calls: &[(u32, Vec<u32>)]) {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[0xdeadbeef]);
    for (index, arguments) in calls {
        for value in arguments {
            instruction(&mut code, 0x25, &[*value]);
        }
        instruction(&mut code, 0x2b, &[*index, arguments.len() as u32 * 4]);
    }
    instruction(&mut code, 0x3b, &[]);
    let id = rt.create(0, kind::ORDINARY, b"sequence", b"").unwrap();
    *rt.object_mut(id).unwrap().interpreter_mut().unwrap() = katze_system2::kcs::Interpreter::new(
        Program::from_code(&code, vec![0x5a; 256], 128).unwrap(),
        vec![],
    )
    .unwrap();
    rt.update(0, 0).unwrap();
    assert_eq!(
        rt.object(id).unwrap().interpreter().unwrap().stack_bytes(),
        0xdeadbeefu32.to_le_bytes()
    );
}

#[test]
fn auxiliary_passes_keep_each_target_capture_and_canvas_independent() {
    for budget in [1, 4096] {
        let mut rt = engine(&[0; 8], true);
        rt.set_budget(budget, 4096);
        let canvas = rt.drawing().clone();
        execute(
            &mut rt,
            &[
                (858, vec![2]),
                (859, vec![0, 10f32.to_bits(), 20f32.to_bits()]),
                (859, vec![1, 0f32.to_bits(), 10f32.to_bits()]),
                (859, vec![2, 0, 30f32.to_bits()]),
                (568, vec![20f32.to_bits(), 21f32.to_bits()]),
            ],
        );
        let targets = rt.compositor().auxiliary_targets();
        for (index, frame) in targets.iter().enumerate() {
            for x in [1, 3, 5] {
                let expected = match (index, x) {
                    (0, 3) => 0xff123457,
                    (1, 1) => 0xff123456,
                    _ => 0,
                };
                assert_eq!(frame.pixel(x, 1), Some(expected));
            }
        }
        assert_eq!(rt.offscreen_frame().unwrap().pixel(5, 1), Some(0xff123458));
        let other = targets[1].clone();
        let capture = rt.offscreen_frame().cloned();
        rt.drawables_mut().set_display(101, 0);
        execute(&mut rt, &[(859, vec![0, 10f32.to_bits(), 20f32.to_bits()])]);
        assert_eq!(rt.compositor().auxiliary_targets()[0].pixel(3, 1), Some(0));
        assert_eq!(rt.compositor().auxiliary_targets()[1], other);
        assert_eq!(rt.offscreen_frame(), capture.as_ref());
        assert_eq!(rt.drawing(), &canvas);
    }
}

#[test]
fn auxiliary_reservations_preserve_capacity_and_reset_grown_targets() {
    let mut rt = engine(&[0; 8], true);
    rt.host_mut().platform_settings = Some(katze_system2::settings::PlatformSettings {
        d3d_backbuffer: 0,
        ..Default::default()
    });
    execute(&mut rt, &[(858, vec![1])]);
    assert!(rt.compositor().auxiliary_targets().is_empty());
    rt.host_mut()
        .platform_settings
        .as_mut()
        .unwrap()
        .d3d_backbuffer = 1;
    execute(
        &mut rt,
        &[(858, vec![1]), (859, vec![0, 0, 30f32.to_bits()])],
    );
    let first = rt.compositor().auxiliary_targets()[0].clone();
    execute(&mut rt, &[(858, vec![0]), (858, vec![1]), (858, vec![3])]);
    assert_eq!(rt.compositor().auxiliary_targets(), &[first]);
    rt.host_mut()
        .platform_settings
        .as_mut()
        .unwrap()
        .d3d_tex_size = 4;
    execute(&mut rt, &[(858, vec![2])]);
    assert!(rt.compositor().auxiliary_targets().is_empty());
    rt.host_mut()
        .platform_settings
        .as_mut()
        .unwrap()
        .d3d_tex_size = u32::MAX;
    execute(&mut rt, &[(858, vec![2])]);
    let targets = rt.compositor().auxiliary_targets();
    assert_eq!(targets.len(), 2);
    for target in targets {
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(target.pixel(x, y), Some(0));
            }
        }
    }
    rt.drawing_mut().enabled = false;
    execute(&mut rt, &[(859, vec![0, 0, 30f32.to_bits()])]);
    assert_eq!(rt.compositor().auxiliary_targets()[0].pixel(3, 1), Some(0));
}

#[test]
fn host_frame_and_script_capture_use_distinct_depth_selections() {
    use std::{cell::RefCell, rc::Rc};

    type Calls = Rc<RefCell<Vec<(DrawList, [u32; 2])>>>;
    struct Backend(Calls);
    impl GraphicsBackend for Backend {
        fn render(
            &mut self,
            frame: &DrawList,
            canvas: &mut FrameCanvas<'_>,
            size: [u32; 2],
        ) -> Result<Surface> {
            self.0.borrow_mut().push((frame.clone(), size));
            SoftwareBackend.render(frame, canvas, size)
        }
    }

    let args: Vec<_> = [10f32, 20.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let mut rt = engine_program(program(&args), true);
    let events = Rc::new(RefCell::new(Vec::new()));
    rt.set_graphics_backend(Backend(events.clone()));
    call(&mut rt);
    let capture = rt.offscreen_frame().unwrap();
    assert_eq!(capture.pixel(1, 1), Some(0));
    assert_eq!(capture.pixel(3, 1), Some(0xff123457));
    assert_eq!(capture.pixel(5, 1), Some(0));
    let (host_frame, rendered) = rt
        .with_frame(|frame, canvas| {
            Ok((
                frame.clone(),
                SoftwareBackend.render(frame, canvas, [8, 4])?,
            ))
        })
        .unwrap();
    assert_eq!(rendered.pixel(1, 1), Some(0xff123456));
    assert_eq!(rendered.pixel(3, 1), Some(0xff123457));
    assert_eq!(rendered.pixel(5, 1), Some(0xff123458));
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, [8, 4]);
    assert_eq!(events[0].0.commands.len(), 2);
    assert_eq!(host_frame.commands.len(), 6);
    assert_eq!(events[0].0.filter, TextureFilter::Point);
    assert_eq!(rt.compose_frame().unwrap(), host_frame);
}

#[test]
fn native_pass_uses_saved_view_ordered_depth_bounds_and_clears_each_output() {
    for budget in [1, 4096] {
        for (range, visible) in [
            ([10.0, 20.0], [false, true, false]),
            ([f32::from_bits(0x7fa12345), 20.0], [true, true, false]),
            ([0.0, f32::from_bits(0xffa23456)], [true; 3]),
            ([0.0, -0.0], [false; 3]),
            ([20.0, 10.0], [false; 3]),
            ([f32::NEG_INFINITY, f32::INFINITY], [true; 3]),
            ([f32::INFINITY, f32::INFINITY], [false; 3]),
        ] {
            for extra in [false, true] {
                let mut args = range
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>();
                if extra {
                    args.extend([0xff; 8]);
                }
                let mut rt = engine(&args, true);
                rt.set_budget(budget, 4096);
                let canvas = rt.drawing().clone();
                call(&mut rt);
                let frame = rt.offscreen_frame().unwrap();
                assert_eq!(frame.size(), [8, 4]);
                let pixels = frame.tiles()[0].as_ref().unwrap();
                for (i, shown) in visible.into_iter().enumerate() {
                    assert_eq!(
                        pixels.pixel(1 + i as u32 * 2, 1),
                        Some(if shown { 0xff123456 + i as u32 } else { 0 })
                    );
                }
                assert_eq!(rt.drawing(), &canvas);
                for id in 100..103 {
                    rt.compositor_mut().drawables_mut().set_display(id, 0);
                }
                call(&mut rt);
                assert!(
                    rt.offscreen_frame().unwrap().tiles()[0]
                        .as_ref()
                        .unwrap()
                        .pixels()
                        .iter()
                        .all(|&b| b == 0)
                );
            }
        }
    }
}

#[test]
fn skipped_passes_keep_the_last_frame_and_short_arguments_precede_gating() {
    let args = [0.0f32, 30.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut rt = engine(&args, true);
    call(&mut rt);
    let previous = rt.offscreen_frame().unwrap().clone();
    rt.drawing_mut().mode = 1;
    call(&mut rt);
    rt.drawing_mut().mode = 0;
    rt.drawing_mut().enabled = false;
    call(&mut rt);
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().target = None;
    call(&mut rt);
    assert_eq!(rt.offscreen_frame(), Some(&previous));

    let mut missing = engine(&args, false);
    missing.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    assert!(missing.update(0, 0).unwrap_err().message.contains("帧视图"));
    assert!(missing.offscreen_frame().is_none());
    let mut short = engine(&args[..7], false);
    short.drawing_mut().enabled = false;
    short.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    assert!(short.update(0, 0).unwrap_err().message.contains("范围"));
    assert!(short.offscreen_frame().is_none());
}

#[test]
fn presentation_needs_no_readback_backend_but_script_capture_does() {
    let args: Vec<_> = [0.0f32, 30.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let mut rt = engine_program(program(&args), true);
    assert!(!rt.compose_frame().unwrap().commands.is_empty());

    rt.drawing_mut().enabled = false;
    call(&mut rt);
    rt.drawing_mut().enabled = true;
    rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    assert!(
        rt.update(0, 0)
            .unwrap_err()
            .message
            .contains("离屏绘制后端")
    );
    assert!(rt.offscreen_frame().is_none());
}

#[test]
fn frame_capture_returns_an_owned_cropped_image_and_skips_a_disabled_device() {
    let mut code = vec![0x2b, 3];
    code.extend(113u32.to_le_bytes());
    code.push(3);
    code.extend(0u32.to_le_bytes());
    code.extend([0x26, 3]);
    code.extend(0u32.to_le_bytes());
    for (opcode, values) in [
        (0x2f, vec![0]),
        (0x25, vec![4]),
        (0x25, vec![8]),
        (0x2b, vec![362, 12]),
    ] {
        code.push(opcode);
        for value in values {
            code.push(3);
            code.extend(u32::to_le_bytes(value));
        }
    }
    code.push(0x3b);
    for enabled in [false, true] {
        let mut rt = engine_program(Program::from_code(&code, vec![0; 16], 128).unwrap(), true);
        if enabled {
            rt.set_graphics_backend(SoftwareBackend);
        }
        rt.drawing_mut().enabled = enabled;
        rt.set_frame_capture_rectangle(Some(katze_system2::graphics::Rect::from_xywh(1, 1, 3, 2)));
        let caller = rt.create(0, kind::ORDINARY, b"capture", b"").unwrap();
        rt.update(0, 0).unwrap();
        let handle = u32::from_le_bytes(
            rt.object(caller)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let extents = rt
            .object(caller)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(4, 8)
            .unwrap()
            .to_vec();
        assert_eq!(
            extents,
            if enabled {
                [3u32.to_le_bytes(), 2u32.to_le_bytes()].concat()
            } else {
                vec![0; 8]
            }
        );
        if enabled {
            let image = rt.images().get(handle).unwrap();
            assert_eq!(image.surface().size(), [3, 2]);
            assert_eq!(image.surface().pixel(0, 0), Some(0xff123456));
            rt.destroy(caller).unwrap();
            assert!(rt.images().get(handle).is_err());
        } else {
            assert_eq!(handle, 0);
        }
    }
}
