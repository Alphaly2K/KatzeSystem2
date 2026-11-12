use katze_system2::{
    Result,
    expression::State,
    graphics::{
        DrawList, Drawable, GraphicsBackend, Projection, RenderTarget, SoftwareBackend,
        TextureFilter, Transform,
    },
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

fn program(args: &[u8]) -> Program {
    fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
        code.push(opcode);
        for word in words {
            code.push(3);
            code.extend(word.to_le_bytes());
        }
    }
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
    engine_program(program(args), view)
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

#[test]
fn capture_and_presentation_share_the_backend_with_distinct_depth_selections() {
    use std::{cell::RefCell, rc::Rc};

    type Calls = Rc<RefCell<Vec<(bool, DrawList, [u32; 2])>>>;
    struct Backend(Calls);
    impl GraphicsBackend for Backend {
        fn render(
            &mut self,
            frame: &DrawList,
            canvas: &mut RenderTarget,
            size: [u32; 2],
        ) -> Result<RenderTarget> {
            self.0.borrow_mut().push((false, frame.clone(), size));
            SoftwareBackend.render(frame, canvas, size)
        }

        fn present(
            &mut self,
            frame: &DrawList,
            _: &mut RenderTarget,
            size: [u32; 2],
        ) -> Result<bool> {
            self.0.borrow_mut().push((true, frame.clone(), size));
            Ok(false)
        }
    }

    let args: Vec<_> = [10f32, 20.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let mut rt = engine(&args, true);
    let events = Rc::new(RefCell::new(Vec::new()));
    rt.set_graphics_backend(Backend(events.clone()));
    call(&mut rt);
    let capture = rt.offscreen_frame().unwrap();
    assert_eq!(capture.pixel(1, 1), Some(0));
    assert_eq!(capture.pixel(3, 1), Some(0xff123457));
    assert_eq!(capture.pixel(5, 1), Some(0));
    assert!(!rt.present_frame([16, 8]).unwrap());
    let events = events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].0, events[0].2), (false, [8, 4]));
    assert_eq!((events[1].0, events[1].2), (true, [16, 8]));
    assert_eq!(events[0].1.commands.len(), 2);
    assert_eq!(events[1].1.commands.len(), 6);
    assert_eq!(events[0].1.filter, TextureFilter::Point);
    assert_eq!(rt.compose_frame().unwrap(), events[1].1);
}

#[test]
fn native_pass_uses_saved_view_ordered_depth_bounds_and_clears_each_output() {
    for (budget, backend) in [(1, false), (4096, false), (1, true), (4096, true)] {
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
                if backend {
                    rt.set_graphics_backend(SoftwareBackend);
                }
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
                    rt.drawables_mut().set_display(id, 0);
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
