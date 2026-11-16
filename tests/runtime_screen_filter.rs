use katze_system2::{
    Result,
    expression::State,
    graphics::{Drawable, GraphicsFactory, Projection, RenderTarget, TextureFilter, Transform},
    kcs::{Interpreter, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::Command,
};

struct Source;
impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(Program::from_code(&[0x3b], vec![], 256)?))
    }
}
type Engine = Runtime<GraphicsFactory<KcsFactory<Source>>>;

fn engine(budget: usize) -> Engine {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Source),
        },
    );
    runtime.set_budget(budget, 1 << 20);
    runtime.host_mut().surface_size = [8, 6];
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    runtime.set_frame_view(
        Projection::default(),
        *Transform::default().matrix(),
        TextureFilter::Linear,
    );
    rectangle(&mut runtime, 100, [0., 0.], [8., 6.], 5., 0xffff0000);
    rectangle(&mut runtime, 101, [0., 0.], [1., 1.], 25., 0xffffff00);
    runtime
}

fn rectangle(runtime: &mut Engine, id: u32, at: [f32; 2], size: [f32; 2], depth: f32, color: u32) {
    runtime
        .drawables_mut()
        .insert(id, Drawable::rectangle([at[0], at[1], 0.], size, color))
        .unwrap();
    runtime.drawables_mut().set_display(id, 1);
    runtime.drawables_mut().set_base_point(id, [0., 0., depth]);
}

fn execute(runtime: &mut Engine, calls: &[(u32, &[u32])], data: Vec<u8>, external: bool) {
    let mut code = vec![0x25, 3];
    code.extend(0xdeadbeefu32.to_le_bytes());
    for (index, arguments) in calls {
        for value in *arguments {
            code.extend([0x25, 3]);
            code.extend(value.to_le_bytes());
        }
        code.extend([0x2b, 3]);
        code.extend(index.to_le_bytes());
        code.push(3);
        code.extend((arguments.len() as u32 * 4).to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(65u32.to_le_bytes());
    code.push(3);
    code.extend(0u32.to_le_bytes());
    let caller = runtime.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    let vm = runtime
        .object_mut(caller)
        .unwrap()
        .interpreter_mut()
        .unwrap();
    *vm = Interpreter::new(
        Program::from_code(&code, data.clone(), 256).unwrap(),
        vec![],
    )
    .unwrap();
    if external {
        vm.replace_external(data).unwrap();
    }
    runtime.update(0, 1).unwrap();
    assert_eq!(
        runtime
            .object(caller)
            .unwrap()
            .interpreter()
            .unwrap()
            .stack_bytes(),
        0xdeadbeefu32.to_le_bytes()
    );
}

fn state(runtime: &mut Engine, target: u32) -> u32 {
    runtime
        .dispatch_command(
            0,
            Command {
                target,
                sender: 0,
                code: 383,
                data: [0; 3],
            },
        )
        .unwrap()
}

fn pixels(runtime: &mut Engine) -> RenderTarget {
    let mut image = RenderTarget::new([8, 6]).unwrap();
    runtime
        .compose_frame()
        .unwrap()
        .render_into(&mut image, runtime.drawing().target.as_ref())
        .unwrap();
    image
}

#[test]
fn blur_natives_preserve_depth_bands_and_reset_feedback_on_every_enable() {
    for budget in [1, 4096] {
        let mut runtime = engine(budget);
        rectangle(&mut runtime, 102, [1., 1.], [2., 2.], 15., 0xff00ff00);
        let effect = runtime.create(0, kind::RENDER_BLUR, b"blur", b"").unwrap();
        let mut data = vec![0; 128];
        for (i, value) in Transform::default().matrix().iter().enumerate() {
            data[64 + i * 4..68 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        execute(
            &mut runtime,
            &[
                (521, &[0]),
                (522, &[0]),
                (523, &[0]),
                (524, &[0]),
                (
                    521,
                    &[
                        effect,
                        0x80000040,
                        1f32.to_bits(),
                        0,
                        0,
                        0x80ffffff,
                        10f32.to_bits(),
                        20f32.to_bits(),
                    ],
                ),
                (522, &[effect, 0x80000001]),
            ],
            data,
            true,
        );
        assert_eq!(state(&mut runtime, effect), 0x80000001);
        let first = runtime.compose_frame().unwrap();
        assert_eq!(pixels(&mut runtime).pixel(1, 1), Some(0xff00ff00));
        runtime.drawables_mut().remove(102);
        rectangle(&mut runtime, 102, [5., 1.], [2., 2.], 15., 0xff0000ff);
        let second = pixels(&mut runtime);
        assert_ne!(second.pixel(2, 1), Some(0xffff0000));
        assert_eq!(second.pixel(5, 1), Some(0xff0000ff));
        assert_eq!(second.pixel(0, 0), Some(0xffffff00));
        let mut snapshot = RenderTarget::new([8, 6]).unwrap();
        first
            .render_into(&mut snapshot, runtime.drawing().target.as_ref())
            .unwrap();
        assert_eq!(snapshot.pixel(1, 1), Some(0xff00ff00));
        assert_eq!(snapshot.pixel(5, 1), Some(0xffff0000));
        execute(&mut runtime, &[(522, &[effect, 1])], vec![], false);
        assert_eq!(pixels(&mut runtime).pixel(2, 1), Some(0xffff0000));
        execute(&mut runtime, &[(522, &[effect, 0])], vec![], false);
        assert_eq!(state(&mut runtime, effect), 0);
        assert_eq!(pixels(&mut runtime).pixel(5, 1), Some(0xff0000ff));
        runtime.destroy(effect).unwrap();
        assert!(runtime.compositor().auxiliary_targets().is_empty());
    }
}

#[test]
fn live_mosaic_keeps_its_first_rectangle_and_uses_point_sampling_inside_linear_frames() {
    for budget in [1, 4096] {
        let mut runtime = engine(budget);
        for x in 0..8 {
            rectangle(
                &mut runtime,
                110 + x,
                [x as f32, 0.],
                [1., 6.],
                15.,
                if x % 2 == 0 { 0xff00ff00 } else { 0xff0000ff },
            );
        }
        let effect = runtime
            .create(0, kind::RENDER_MOSAIC, b"mosaic", b"")
            .unwrap();
        execute(&mut runtime, &[(524, &[effect, 7])], vec![], false);
        assert_eq!(state(&mut runtime, effect), 7);
        assert_eq!(pixels(&mut runtime).pixel(2, 2), Some(0xff00ff00));
        execute(
            &mut runtime,
            &[(
                523,
                &[
                    effect,
                    1f32.to_bits(),
                    1f32.to_bits(),
                    4f32.to_bits(),
                    4f32.to_bits(),
                    2f32.to_bits(),
                    2f32.to_bits(),
                    10f32.to_bits(),
                    20f32.to_bits(),
                    u32::MAX,
                ],
            )],
            vec![],
            false,
        );
        let frame = runtime.compose_frame().unwrap();
        let filtered = &frame.commands[2..4];
        assert!(
            filtered
                .iter()
                .all(|command| command.texture.as_ref().unwrap().filter
                    == Some(TextureFilter::Point))
        );
        let geometry: Vec<_> = filtered
            .iter()
            .map(|command| command.vertices.map(|v| v.position))
            .collect();
        assert_eq!(
            geometry[0],
            [[0.5, 0.5, 0.5], [4.5, 0.5, 0.5], [0.5, 4.5, 0.5]]
        );
        let image = pixels(&mut runtime);
        assert_eq!(image.pixel(0, 0), Some(0xffffff00));
        assert_eq!(image.pixel(7, 5), Some(0xffff0000));
        assert_ne!(image.pixel(2, 2), Some(0xffff0000));
        execute(
            &mut runtime,
            &[
                (524, &[effect, 0]),
                (
                    523,
                    &[
                        effect,
                        3f32.to_bits(),
                        3f32.to_bits(),
                        2f32.to_bits(),
                        2f32.to_bits(),
                        0,
                        0,
                        10f32.to_bits(),
                        20f32.to_bits(),
                        u32::MAX,
                    ],
                ),
                (524, &[effect, 9]),
            ],
            vec![],
            false,
        );
        let frame = runtime.compose_frame().unwrap();
        assert_eq!(
            frame.commands[2..4]
                .iter()
                .map(|command| command.vertices.map(|v| v.position))
                .collect::<Vec<_>>(),
            geometry
        );
        assert_eq!(pixels(&mut runtime).pixel(2, 2), Some(0xffff0000));
        runtime.destroy(effect).unwrap();
        assert!(runtime.compositor().auxiliary_targets().is_empty());
        assert_eq!(pixels(&mut runtime).pixel(2, 2), Some(0xff00ff00));
    }
}

#[test]
fn unavailable_filter_targets_ignore_configuration_and_remain_inactive() {
    for object_kind in [kind::RENDER_BLUR, kind::RENDER_MOSAIC] {
        let mut runtime = engine(1);
        runtime.host_mut().platform_settings = Some(katze_system2::settings::PlatformSettings {
            d3d_backbuffer: 0,
            ..Default::default()
        });
        let effect = runtime.create(0, object_kind, b"unavailable", b"").unwrap();
        for (code, data) in [
            (
                if object_kind == kind::RENDER_BLUR {
                    384
                } else {
                    386
                },
                [0; 3],
            ),
            (
                if object_kind == kind::RENDER_BLUR {
                    385
                } else {
                    387
                },
                [0, u32::MAX, 0],
            ),
        ] {
            runtime
                .dispatch_command(
                    0,
                    Command {
                        target: effect,
                        sender: 0,
                        code,
                        data,
                    },
                )
                .unwrap();
        }
        assert_eq!(state(&mut runtime, effect), 0);
        assert_eq!(pixels(&mut runtime).pixel(2, 2), Some(0xffff0000));
        runtime.destroy(effect).unwrap();
    }
}
