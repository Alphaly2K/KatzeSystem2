use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, Projection, RenderTarget, TextureFilter, Transform},
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
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Source(program(args))),
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
