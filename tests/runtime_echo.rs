use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, GraphicsFactory, RenderTarget},
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

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn instruction(code: &mut Vec<u8>, op: u8, words: &[u32]) {
    code.push(op);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, words: &[u32]) {
    for word in words {
        instruction(code, 0x25, &[*word]);
    }
    instruction(code, 0x2b, &[index, words.len() as u32 * 4]);
}

fn command(code: u32, data0: u32) -> Command {
    Command {
        target: 1,
        sender: 0,
        code,
        data: [data0, 0, 0],
    }
}

#[test]
fn ordinary_echo_calls_restore_delayed_attributes_and_preserve_the_return_slot() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        instruction(&mut code, 0x25, &[0xdeadbeef]);
        call(&mut code, 398, &[0; 12]);
        call(
            &mut code,
            398,
            &[
                1,
                2,
                1,
                1f32.to_bits(),
                2f32.to_bits(),
                3f32.to_bits(),
                64,
                0.5f32.to_bits(),
                0.5f32.to_bits(),
                1f32.to_bits(),
                0,
                (-10i32) as u32,
            ],
        );
        call(&mut code, 399, &[1, 32, 44, 56, 68, 0xc0112233, 7]);
        instruction(&mut code, 0x26, &[0]);
        instruction(&mut code, 0x3b, &[]);
        let mut data = vec![0; 160];
        for (offset, values) in [
            (32, [4f32, 5., 900.]),
            (44, [10., 20., 30.]),
            (56, [2., 4., 1.]),
        ] {
            data[offset..offset + 12].copy_from_slice(&bytes(&values.map(f32::to_bits)));
        }
        let matrix =
            std::array::from_fn::<_, 16, _>(|i| if i % 5 == 0 { 1f32.to_bits() } else { 0 });
        data[68..132].copy_from_slice(&bytes(&matrix));
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Loader(Program::from_code(&code, data, 256).unwrap())),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
        runtime
            .drawables_mut()
            .insert(1, Drawable::default())
            .unwrap();
        runtime.drawables_mut().set_display(1, 1);
        let task = runtime.create(0, kind::ORDINARY, b"echo", b"").unwrap();
        runtime.update(1, 1).unwrap();
        assert_eq!(runtime.drawables().get(0x10001).unwrap().display(), 0);
        assert_eq!(
            runtime
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap(),
            0xdeadbeefu32.to_le_bytes()
        );
        runtime.update(2, 1).unwrap();
        let copy = runtime.drawables().get(0x10001).unwrap();
        assert_eq!(copy.transform().base_point(), [4., 5., 890.]);
        assert_eq!(copy.transform().position(), [11., 22., 33.]);
        assert_eq!(copy.transform().scale(), [1.5, 3., 1.]);
        assert_eq!(copy.color(), (0x80112233, 1));
        assert_eq!(copy.display(), 7);
        runtime.dispatch_command(0, command(47, 0)).unwrap();
        runtime.drawables_mut().set_position(1, [99., 98., 97.]);
        runtime.update(3, 1).unwrap();
        let copy = runtime.drawables().get(0x10001).unwrap();
        assert_eq!(copy.display(), 0);
        assert_eq!(copy.transform().position(), [11., 22., 33.]);
        let mut show = command(47, 0);
        show.data[1] = 1;
        runtime.dispatch_command(0, show).unwrap();
        runtime.update(4, 1).unwrap();
        assert_eq!(
            runtime
                .drawables()
                .get(0x10001)
                .unwrap()
                .transform()
                .position(),
            [1., 2., 3.]
        );
        runtime.update(5, 1).unwrap();
        assert_eq!(
            runtime
                .drawables()
                .get(0x10001)
                .unwrap()
                .transform()
                .position(),
            [100., 100., 100.]
        );
        runtime.dispatch_command(0, command(79, 0)).unwrap();
        assert!(runtime.drawables().get(0x10001).is_none());
        assert!(runtime.drawables().get(0x10002).is_none());
    }
}

#[test]
fn ribbon_echo_slices_canvas_sources_and_releases_copies_without_rewinding_the_plane() {
    let program = Program::from_code(&[0x3b], Vec::new(), 4).unwrap();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Loader(program)),
        },
    );
    runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    runtime.drawing_mut().target = Some(RenderTarget::new([128, 128]).unwrap());
    let mut rectangle = bytes(&[0, 0, 0, 100f32.to_bits(), 80f32.to_bits(), 0, 0, 0]);
    runtime
        .dispatch_command_with_buffers(
            0,
            command(41, 0),
            &mut CommandBuffers {
                data1: Some(&mut rectangle),
                ..Default::default()
            },
        )
        .unwrap();
    let mut config = bytes(&[
        3,
        7,
        0,
        1f32.to_bits(),
        1f32.to_bits(),
        1f32.to_bits(),
        0,
        0,
        0,
        0,
        0,
        4,
        0,
    ]);
    runtime
        .dispatch_command_with_buffers(
            0,
            command(79, 1),
            &mut CommandBuffers {
                data1: Some(&mut config),
                ..Default::default()
            },
        )
        .unwrap();
    let source = |id| {
        runtime
            .drawables()
            .get(id)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
    };
    assert_eq!(source(1).scale(), [1., 0.5375, 1.]);
    assert_eq!(source(0x10001).scale(), [1., 0.025, 1.]);
    assert_eq!(source(0x10001).position(), [0., 39., 0.]);
    assert_eq!(source(0x10003).position(), [0., 41., 0.]);
    runtime.drawables_mut().set_position(1, [30., 40., 0.]);
    let main_color = runtime.drawables().get(1).unwrap().color();
    runtime.update(1, 1).unwrap();
    assert_eq!(runtime.drawables().get(1).unwrap().color(), main_color);
    runtime.dispatch_command(0, command(79, 0)).unwrap();
    assert_eq!(
        runtime.drawables().get(1).unwrap().transform().position(),
        [30., 40., 0.]
    );
    assert_eq!(runtime.drawables_mut().order(), &[1]);
    runtime
        .dispatch_command_with_buffers(
            0,
            command(79, 1),
            &mut CommandBuffers {
                data1: Some(&mut config),
                ..Default::default()
            },
        )
        .unwrap();
    runtime.destroy(1).unwrap();
    assert!(runtime.drawables_mut().order().is_empty());
}
