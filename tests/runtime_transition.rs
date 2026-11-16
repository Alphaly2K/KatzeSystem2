use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, GraphicsFactory, Projection, RenderTarget, TextureFilter},
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

#[test]
fn wave_natives_change_bound_vertices_and_release_without_changing_source_geometry() {
    const IDENTITY: [f32; 16] = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    for budget in [1, 4096] {
        let mut code = Vec::new();
        instruction(&mut code, 0x25, &[0xdead_beef]);
        call(&mut code, 196, &[1, 64]);
        call(&mut code, 197, &[1]);
        call(&mut code, 200, &[1]);
        instruction(&mut code, 0x26, &[0]);
        call(&mut code, 65, &[]);
        call(&mut code, 201, &[0, 0, u32::MAX]);
        call(&mut code, 201, &[1, 1, u32::MAX]);
        call(&mut code, 201, &[1, 0, 96]);
        call(&mut code, 65, &[]);
        call(&mut code, 198, &[1]);
        call(&mut code, 200, &[1]);
        instruction(&mut code, 0x26, &[4]);
        call(&mut code, 65, &[]);
        call(&mut code, 197, &[1]);
        call(&mut code, 65, &[]);
        instruction(&mut code, 0x3b, &[]);
        let mut data = vec![0; 128];
        for (at, target, axis, amplitude) in [(64, 2, 1, 3f32), (96, 99, 0, 6.)] {
            let record: Vec<_> = [
                target,
                axis,
                amplitude.to_bits(),
                40f32.to_bits(),
                60f32.to_bits(),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            data[at..at + record.len()].copy_from_slice(&record);
        }
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Loader(Program::from_code(&code, data, 128).unwrap())),
            },
        );
        rt.set_budget(budget, 1 << 20);
        assert_eq!(rt.create(0, kind::WAVE, b"wave", b"").unwrap(), 1);
        assert_eq!(rt.create(0, kind::PLANE, b"geometry", b"").unwrap(), 2);
        let caller = rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
        rt.drawables_mut().remove(2);
        rt.drawables_mut()
            .insert(2, Drawable::rectangle([0., 10., 0.], [10., 10.], u32::MAX))
            .unwrap();
        rt.drawables_mut().set_display(2, 1);
        let source = rt.drawables().get(2).unwrap().triangles().to_vec();
        let frame = |rt: &mut Runtime<_>| {
            rt.drawables_mut()
                .compose(&Projection::default(), IDENTITY, TextureFilter::Point, None)
                .unwrap()
        };
        let baseline = frame(&mut rt);
        assert_eq!(baseline.commands.len(), 2);
        rt.update(0, 1).unwrap();
        let changed = frame(&mut rt);
        let x = baseline.commands[0].vertices[0].position[0];
        assert!((changed.commands[0].vertices[0].position[0] - x - 3.).abs() < 0.000001);
        rt.update(1, 1).unwrap();
        let changed = frame(&mut rt);
        let offset = ((11f32 * std::f32::consts::PI * 2. / 40.) as f64).sin() as f32 * 6.;
        assert!((changed.commands[0].vertices[0].position[0] - x - offset).abs() < 0.00001);
        rt.update(2, 1).unwrap();
        assert_eq!(frame(&mut rt), baseline);
        rt.update(3, 1).unwrap();
        assert!((frame(&mut rt).commands[0].vertices[0].position[0] - x - 6.).abs() < 0.000001);
        assert_eq!(rt.drawables().get(2).unwrap().triangles(), source);
        let vm = rt.object(caller).unwrap().interpreter().unwrap();
        assert_eq!(
            vm.read(0, 8).unwrap(),
            [1u32.to_le_bytes(), 0u32.to_le_bytes()].concat()
        );
        assert_eq!(vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
        rt.destroy(1).unwrap();
        assert_eq!(frame(&mut rt), baseline);
    }
}

#[test]
fn wave_canvas_vertices_use_local_coordinates_and_shared_binding_lifetimes() {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Loader(Program::from_code(&[0x3b], vec![], 128).unwrap())),
        },
    );
    let plane = rt.create(0, kind::PLANE, b"canvas", b"").unwrap();
    let a = rt.create(0, kind::WAVE, b"first", b"").unwrap();
    let b = rt.create(0, kind::WAVE, b"second", b"").unwrap();
    let mut bytes = Vec::new();
    for points in [
        [[2f32, 13f32], [12., 13.], [12., 23.]],
        [[2f32, 13f32], [2., 23.], [12., 23.]],
    ] {
        bytes.extend([0u8; 8]);
        for point in points {
            for value in [point[0].to_bits(), point[1].to_bits(), 0, u32::MAX, 0, 0] {
                bytes.extend(value.to_le_bytes());
            }
        }
    }
    rt.drawables_mut().remove(plane);
    rt.drawables_mut()
        .insert(plane, Drawable::from_canvas(&bytes, 2).unwrap())
        .unwrap();
    rt.drawables_mut().set_display(plane, 1);
    rt.drawables_mut().set_base_point(plane, [2., 3., 5000.]);
    rt.drawables_mut().set_scale(plane, [2., 1., 1.]);
    rt.drawables_mut().set_position(plane, [10., 20., 0.]);
    let source = rt
        .drawables()
        .get(plane)
        .unwrap()
        .canvas_triangles()
        .unwrap()
        .to_vec();
    let original = rt.drawables().get(plane).unwrap().clone();
    let identity = std::array::from_fn(|i| f32::from(i % 5 == 0));
    let frame = |rt: &mut Runtime<_>| {
        rt.drawables_mut()
            .compose(&Projection::default(), identity, TextureFilter::Point, None)
            .unwrap()
    };
    let baseline = frame(&mut rt);
    assert_eq!(baseline.commands.len(), 2);
    let command = |target, code| Command {
        target,
        sender: 0,
        code,
        data: [0; 3],
    };
    for (id, amplitude) in [(a, 3f32), (b, 6.)] {
        let mut parameters: Vec<_> = [
            plane,
            1,
            amplitude.to_bits(),
            40f32.to_bits(),
            60f32.to_bits(),
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        rt.dispatch_command_with_buffers(
            0,
            command(id, 269),
            &mut CommandBuffers {
                data1: Some(&mut parameters),
                ..Default::default()
            },
        )
        .unwrap();
        rt.dispatch_command(0, command(id, 263)).unwrap();
        let changed = frame(&mut rt);
        assert!(
            (changed.commands[0].vertices[0].position[0]
                - baseline.commands[0].vertices[0].position[0]
                - amplitude * 2.)
                .abs()
                < 0.000001
        );
        assert_eq!(changed.commands[0].texture, baseline.commands[0].texture);
    }
    rt.dispatch_command(0, command(a, 264)).unwrap();
    assert_eq!(frame(&mut rt), baseline);
    assert_eq!(rt.dispatch_command(0, command(b, 265)).unwrap(), 1);
    rt.dispatch_command(0, command(b, 263)).unwrap();
    rt.destroy(a).unwrap();
    assert_ne!(frame(&mut rt), baseline);
    rt.drawables_mut().remove(plane);
    rt.drawables_mut().insert(plane, original).unwrap();
    rt.update(1, 1).unwrap();
    assert_eq!(frame(&mut rt), baseline);
    rt.dispatch_command(0, command(b, 263)).unwrap();
    assert_ne!(frame(&mut rt), baseline);
    rt.destroy(b).unwrap();
    assert_eq!(frame(&mut rt), baseline);
    assert_eq!(
        rt.drawables()
            .get(plane)
            .unwrap()
            .canvas_triangles()
            .unwrap(),
        source
    );
}

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for &word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, arguments: &[u32]) {
    for &argument in arguments {
        instruction(code, 0x25, &[argument]);
    }
    instruction(code, 0x2b, &[index, arguments.len() as u32 * 4]);
}

#[test]
fn ordinary_transition_calls_prepare_play_query_and_finish_image_effects() {
    for budget in [1, 4096] {
        for effect in [kind::WIPE, kind::FADE, kind::WIPE_ALPHA] {
            let mut code = Vec::new();
            instruction(&mut code, 0x25, &[0xdeadbeef]);
            call(&mut code, 196, &[0, u32::MAX]);
            call(&mut code, 196, &[1, 64]);
            call(&mut code, 197, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[0]);
            call(&mut code, 65, &[]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[4]);
            call(&mut code, 199, &[1, 128]);
            call(&mut code, 203, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[16]);
            call(&mut code, 197, &[1]);
            call(&mut code, 198, &[1]);
            call(&mut code, 200, &[1]);
            instruction(&mut code, 0x26, &[8]);
            instruction(&mut code, 0x26, &[12]);
            instruction(&mut code, 0x3b, &[]);

            let mut data = vec![0; 256];
            data[128..192].fill(0x75);
            let mut rt = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                GraphicsFactory {
                    fallback: KcsFactory::new(Loader(
                        Program::from_code(&code, data, 256).unwrap(),
                    )),
                },
            );
            rt.set_budget(budget, 1 << 20);
            rt.drawing_mut().enabled = true;
            rt.drawing_mut().active = true;
            rt.drawing_mut().target = Some(RenderTarget::new([3, 3]).unwrap());
            let mut handles = Vec::new();
            for color in [0xff000000u32, 0xff806040, 0xff000080] {
                let handle = rt.images_mut().create(0, [1, 1]).unwrap();
                handles.push(handle);
                rt.images_mut()
                    .get_mut(handle)
                    .unwrap()
                    .pixels_mut()
                    .copy_from_slice(&color.to_le_bytes());
            }
            assert_eq!(rt.create(0, effect, b"effect", b"").unwrap(), 1);
            assert_eq!(rt.create(0, kind::ORDINARY, b"caller", b"").unwrap(), 2);
            let parameters: Vec<_> = match effect {
                kind::WIPE => vec![
                    2, handles[0], 0, 0, handles[1], 0, 0, handles[2], 1, 1, 1, 1, 0, 0,
                ],
                kind::FADE => vec![2, handles[0], 0, 0, handles[1], 0, 0, 1, 1, 1, 1],
                _ => vec![2, handles[1], 0, 0, handles[2], 1, 1, 1, 1, 1, 0, 0],
            }
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            rt.object_mut(2)
                .unwrap()
                .interpreter_mut()
                .unwrap()
                .write(64, &parameters)
                .unwrap();
            assert!(rt.update(10, 1).unwrap());
            assert!(rt.update(11, 1).unwrap());
            let vm = rt.object(2).unwrap().interpreter().unwrap();
            for (address, expected) in [(0, 1u32), (4, 1), (8, 0), (12, 0xdeadbeef), (16, 0)] {
                assert_eq!(
                    vm.read(address, 4).unwrap().as_ref(),
                    &expected.to_le_bytes()
                );
            }
            assert_eq!(vm.read(64, parameters.len()).unwrap().as_ref(), parameters);
            assert_eq!(vm.read(128, 64).unwrap().as_ref(), &[0x75; 64]);
            assert_eq!(
                rt.drawing().target.as_ref().unwrap().pixel(1, 1),
                Some(0xff806040)
            );
        }
    }
}
