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
        Ok(Some(Program::from_code(&[0x3b], vec![], 128)?))
    }
}
type Engine = Runtime<GraphicsFactory<KcsFactory<Source>>>;

fn engine() -> Engine {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Source),
        },
    );
    rt.host_mut().surface_size = [8, 6];
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    rt.set_frame_view(
        Projection::default(),
        *Transform::default().matrix(),
        TextureFilter::Point,
    );
    rt.drawables_mut()
        .insert(100, Drawable::rectangle([0.; 3], [8., 6.], 0xff11_2233))
        .unwrap();
    rt.drawables_mut().set_display(100, 1);
    rt.drawables_mut().set_base_point(100, [0., 0., 5.]);
    rt
}

fn execute(rt: &mut Engine, calls: &[(u32, &[u32])]) {
    execute_at(rt, calls, 0);
}

fn execute_at(rt: &mut Engine, calls: &[(u32, &[u32])], now: u32) {
    let mut code = vec![];
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
    code.push(0x3b);
    let caller = rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    *rt.object_mut(caller).unwrap().interpreter_mut().unwrap() =
        Interpreter::new(Program::from_code(&code, vec![], 128).unwrap(), vec![]).unwrap();
    rt.update(now, 0).unwrap();
}

fn state(rt: &mut Engine, id: u32) -> u32 {
    rt.dispatch_command(
        0,
        Command {
            target: id,
            sender: 0,
            code: 383,
            data: [0; 3],
        },
    )
    .unwrap()
}

fn capture(rt: &mut Engine) {
    execute(rt, &[(858, &[2]), (859, &[0, 0, 10f32.to_bits()])]);
    rt.drawables_mut().set_color(100, 0xffab_cdef, u32::MAX);
    execute(rt, &[(859, &[1, 0, 10f32.to_bits()])]);
}

#[test]
fn native_mosaic_captures_both_scenes_preserves_foreground_and_releases_its_registration() {
    for algorithm in [0, 1] {
        let mut rt = engine();
        capture(&mut rt);
        let id = rt.create(0, kind::RENDER_WMOSAIC, b"effect", b"").unwrap();
        execute(
            &mut rt,
            &[
                (853, &[id, 8, 0, 10f32.to_bits(), 5, algorithm]),
                (854, &[id]),
                (855, &[id, 7]),
            ],
        );
        assert!(rt.compositor().auxiliary_targets().is_empty());
        rt.drawables_mut()
            .insert(
                101,
                Drawable::rectangle([2., 2., 0.], [2., 2.], 0xff55_7733),
            )
            .unwrap();
        rt.drawables_mut().set_display(101, 1);
        rt.drawables_mut().set_base_point(101, [0., 0., 20.]);
        assert_eq!(state(&mut rt, id), 7);
        let frame = rt.compose_frame().unwrap();
        let mut output = RenderTarget::new([8, 6]).unwrap();
        frame
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xff11_2233));
        assert_eq!(output.pixel(2, 2), Some(0xff55_7733));
        rt.update(4, 1).unwrap();
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.render_frame(
            &mut output,
            &Projection::default(),
            *Transform::default().matrix(),
            TextureFilter::Point,
        )
        .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xffab_cdef));
        assert_eq!(output.pixel(2, 2), Some(0xff55_7733));
        rt.update(8, 1).unwrap();
        rt.compose_frame().unwrap();
        assert_eq!(state(&mut rt, id), 0);
        rt.drawables_mut().set_color(100, 0xff33_4466, u32::MAX);
        rt.destroy(id).unwrap();
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xff33_4466));
    }
}

#[test]
fn unavailable_targets_and_short_duration_leave_the_live_scene_visible() {
    for mode in 0..4 {
        let unavailable = mode != 0;
        let mut rt = engine();
        if mode == 0 || mode == 2 {
            capture(&mut rt);
        } else if mode == 3 {
            execute(&mut rt, &[(858, &[1])]);
        }
        if unavailable {
            rt.host_mut().platform_settings = Some(katze_system2::settings::PlatformSettings {
                d3d_backbuffer: u32::from(mode == 3),
                d3d_tex_size: if mode == 3 { 4 } else { u32::MAX },
                ..Default::default()
            });
        }
        let id = rt.create(0, kind::RENDER_WMOSAIC, b"effect", b"").unwrap();
        execute(
            &mut rt,
            &[
                (
                    853,
                    &[
                        id,
                        if unavailable { 8 } else { 1 },
                        0,
                        10f32.to_bits(),
                        5,
                        0,
                    ],
                ),
                (854, &[id]),
                (855, &[id, 1]),
                (853, &[0]),
                (854, &[0]),
                (855, &[0]),
            ],
        );
        assert_eq!(state(&mut rt, id), 0);
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(
            output.pixel(0, 0),
            Some(if mode == 0 || mode == 2 {
                0xffab_cdef
            } else {
                0xff11_2233
            })
        );
        rt.destroy(id).unwrap();
    }
}

#[test]
fn fading_mosaic_captures_during_creation_then_blends_over_the_live_scene() {
    for (algorithm, duration) in [(0, 8), (1, 8), (0, 0), (1, 1)] {
        let mut rt = engine();
        execute(&mut rt, &[(858, &[1]), (859, &[0, 0, 10f32.to_bits()])]);
        let id = rt.create(0, kind::RENDER_FMOSAIC, b"fade", b"").unwrap();
        assert!(rt.compositor().auxiliary_targets().is_empty());
        rt.drawables_mut().set_color(100, 0xffab_cdef, u32::MAX);
        execute(
            &mut rt,
            &[
                (
                    856,
                    &[id, duration, 0, 10f32.to_bits(), 5, algorithm, u32::MAX],
                ),
                (857, &[id, 3]),
                (856, &[0]),
                (857, &[0]),
            ],
        );
        if duration < 2 {
            assert_eq!(state(&mut rt, id), 3);
            rt.update(duration, 1).unwrap();
            rt.compose_frame().unwrap();
            assert_eq!(state(&mut rt, id), 0);
            rt.destroy(id).unwrap();
            continue;
        }
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xff11_2233));
        rt.update(2, 1).unwrap();
        let frame = rt.compose_frame().unwrap();
        assert!(
            frame
                .commands
                .iter()
                .any(|command| command.vertices[0].color == 0xc0ff_ffff)
        );
        let mut output = RenderTarget::new([8, 6]).unwrap();
        frame
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xd037_4c61));
        assert_eq!(state(&mut rt, id), 3);
        rt.update(8, 1).unwrap();
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xffab_cdef));
        assert_eq!(state(&mut rt, id), 0);
        rt.destroy(id).unwrap();
    }
}

#[test]
fn shatter_uses_the_captured_scene_keeps_foreground_and_releases_its_target() {
    let mut rt = engine();
    execute(&mut rt, &[(858, &[1]), (859, &[0, 0, 10f32.to_bits()])]);
    let id = rt.create(0, kind::RENDER_WBREAK, b"shatter", b"").unwrap();
    execute(
        &mut rt,
        &[
            (860, &[id, 8, 0, 10f32.to_bits(), 3, 4, 0, 0]),
            (861, &[id, 7]),
            (860, &[0]),
            (861, &[0]),
        ],
    );
    assert_eq!(rt.compositor().auxiliary_targets().len(), 1);
    rt.drawables_mut()
        .insert(101, Drawable::rectangle([2., 2., 0.], [2., 2.], 0xff557733))
        .unwrap();
    rt.drawables_mut().set_display(101, 1);
    rt.drawables_mut().set_base_point(101, [0., 0., 20.]);
    assert_eq!(state(&mut rt, id), 7);
    for color in [0xff112233, 0xffabcdef] {
        rt.drawables_mut().set_color(100, color, u32::MAX);
        let frame = rt.compose_frame().unwrap();

        let mut output = RenderTarget::new([8, 6]).unwrap();
        frame
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        assert_eq!(output.pixel(0, 0), Some(0xff112233));
        assert_eq!(output.pixel(2, 2), Some(0xff557733));
    }
    execute(&mut rt, &[(859, &[0, 0, 10f32.to_bits()])]);
    let frame = rt.compose_frame().unwrap();
    let mut output = RenderTarget::new([8, 6]).unwrap();
    frame
        .render_into(&mut output, rt.drawing().target.as_ref())
        .unwrap();
    assert_eq!(output.pixel(0, 0), Some(0xffabcdef));
    rt.update(8, 1).unwrap();
    let frame = rt.compose_frame().unwrap();
    assert_eq!(state(&mut rt, id), 0);
    let mut output = RenderTarget::new([8, 6]).unwrap();
    frame
        .render_into(&mut output, rt.drawing().target.as_ref())
        .unwrap();
    assert_eq!(output.pixel(0, 0), Some(0xffabcdef));
    assert_eq!(output.pixel(2, 2), Some(0xff557733));
    let fading = rt.create(0, kind::RENDER_FMOSAIC, b"fade", b"").unwrap();
    execute(&mut rt, &[(856, &[fading, 8, 0, 10f32.to_bits(), 2, 0, 0])]);
    rt.drawables_mut().set_color(100, 0xff224466, u32::MAX);
    let frame = rt.compose_frame().unwrap();
    let mut output = RenderTarget::new([8, 6]).unwrap();
    frame
        .render_into(&mut output, rt.drawing().target.as_ref())
        .unwrap();
    assert_eq!(output.pixel(0, 0), Some(0xffabcdef));
    rt.destroy(fading).unwrap();
    rt.destroy(id).unwrap();
    assert!(rt.compositor().auxiliary_targets().is_empty());
    let frame = rt.compose_frame().unwrap();

    frame
        .render_into(&mut output, rt.drawing().target.as_ref())
        .unwrap();
    assert_eq!(output.pixel(0, 0), Some(0xff224466));
}

#[test]
fn page_flip_captures_refreshes_and_releases_the_scene_with_foreground_preserved() {
    let mut rt = engine();
    execute(&mut rt, &[(858, &[1]), (859, &[0, 0, 10f32.to_bits()])]);
    let id = rt.create(0, kind::PFLIP, b"page", b"").unwrap();
    execute(
        &mut rt,
        &[
            (
                967,
                &[
                    id,
                    8,
                    0,
                    2f32.to_bits(),
                    std::f32::consts::PI.to_bits(),
                    123,
                    10f32.to_bits(),
                ],
            ),
            (968, &[id, 7]),
            (969, &[id]),
            (967, &[0]),
            (968, &[0]),
            (969, &[0]),
        ],
    );
    rt.drawables_mut().set_color(100, 0xffabcdef, u32::MAX);
    rt.drawables_mut()
        .insert(101, Drawable::rectangle([2., 2., 0.], [2., 2.], 0xff557733))
        .unwrap();
    rt.drawables_mut().set_display(101, 1);
    rt.drawables_mut().set_base_point(101, [0., 0., 20.]);
    let render = |rt: &mut Engine| {
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        output
    };
    assert_eq!(state(&mut rt, id), 7);
    let initial = render(&mut rt);
    assert_eq!(initial.pixel(0, 0), Some(0xff112233));
    assert_eq!(initial.pixel(2, 2), Some(0xff557733));
    execute(&mut rt, &[(859, &[0, 0, 10f32.to_bits()])]);
    assert_eq!(render(&mut rt).pixel(0, 0), Some(0xffabcdef));
    rt.update(4, 1).unwrap();
    let curled = render(&mut rt);
    assert_eq!(state(&mut rt, id), 7);
    assert_eq!(curled.pixel(2, 2), Some(0xff557733));
    execute_at(&mut rt, &[(968, &[id, 0])], 4);
    rt.update(6, 1).unwrap();
    assert_eq!(render(&mut rt), curled);
    execute_at(&mut rt, &[(968, &[id, 3])], 6);
    rt.update(14, 1).unwrap();
    assert_eq!(state(&mut rt, id), 3);
    render(&mut rt);
    assert_eq!(state(&mut rt, id), 0);
    rt.destroy(id).unwrap();
    assert!(rt.compositor().auxiliary_targets().is_empty());
    assert_eq!(render(&mut rt).pixel(0, 0), Some(0xffabcdef));
}

#[test]
fn cell_flip_uses_both_captures_keeps_foreground_and_releases_the_targets() {
    let mut rt = engine();
    capture(&mut rt);
    let id = rt.create(0, kind::CFLIP, b"cells", b"").unwrap();
    execute(
        &mut rt,
        &[
            (970, &[id]),
            (
                971,
                &[
                    id,
                    8,
                    3,
                    2,
                    0,
                    3f32.to_bits(),
                    std::f32::consts::FRAC_PI_2.to_bits(),
                    0xff778899,
                    123,
                    10f32.to_bits(),
                ],
            ),
            (972, &[id, 7]),
            (973, &[id]),
            (970, &[0]),
            (971, &[0]),
            (972, &[0]),
            (973, &[0]),
        ],
    );
    rt.drawables_mut().set_color(100, 0xff224466, u32::MAX);
    rt.drawables_mut()
        .insert(101, Drawable::rectangle([2., 2., 0.], [2., 2.], 0xff557733))
        .unwrap();
    rt.drawables_mut().set_display(101, 1);
    rt.drawables_mut().set_base_point(101, [0., 0., 20.]);
    let render = |rt: &mut Engine| {
        let mut output = RenderTarget::new([8, 6]).unwrap();
        rt.compose_frame()
            .unwrap()
            .render_into(&mut output, rt.drawing().target.as_ref())
            .unwrap();
        output
    };
    assert_eq!(state(&mut rt, id), 7);
    assert_eq!(render(&mut rt).pixel(0, 0), Some(0xff112233));
    rt.update(2, 1).unwrap();
    let flipped = render(&mut rt);
    assert_eq!(flipped.pixel(0, 0), Some(0xff778899));
    assert_eq!(flipped.pixel(2, 2), Some(0xff557733));
    rt.update(8, 1).unwrap();
    assert_eq!(state(&mut rt, id), 7);
    let finished = render(&mut rt);
    assert_eq!(state(&mut rt, id), 0);
    assert_eq!(finished.pixel(0, 0), Some(0xffabcdef));
    execute_at(&mut rt, &[(859, &[1, 0, 10f32.to_bits()])], 8);
    assert_eq!(render(&mut rt).pixel(0, 0), Some(0xff224466));
    rt.destroy(id).unwrap();
    assert!(rt.compositor().auxiliary_targets().is_empty());
    assert_eq!(render(&mut rt).pixel(0, 0), Some(0xff224466));
}

#[test]
fn render_wipe_uses_image_wrappers_and_keeps_foreground_and_stopped_pixels() {
    for native in [530, 905] {
        let mut rt = engine();
        let mask = rt.images_mut().create(0, [8, 6]).unwrap();
        rt.images_mut()
            .get_mut(mask)
            .unwrap()
            .fill_rect_from_bottom(
                katze_system2::graphics::Rect::from_xywh(0, 0, 8, 6),
                0xff20_1080,
            );
        execute(&mut rt, &[(858, &[1]), (859, &[0, 0, 10f32.to_bits()])]);
        let id = rt.create(0, kind::RWIPE, b"wipe", b"").unwrap();
        assert!(rt.compositor().auxiliary_targets().is_empty());
        rt.drawables_mut().set_color(100, 0xffab_cdef, u32::MAX);
        rt.drawables_mut()
            .insert(
                101,
                Drawable::rectangle([2., 2., 0.], [2., 2.], 0xff55_7733),
            )
            .unwrap();
        rt.drawables_mut().set_display(101, 1);
        rt.drawables_mut().set_base_point(101, [0., 0., 20.]);
        execute(
            &mut rt,
            &[
                (native, &[id, 8, 123f32.to_bits(), 10f32.to_bits(), mask]),
                (531, &[id, 7]),
                (native, &[0]),
                (native, &[id, 0, 0, 0, 0]),
                (532, &[id]),
            ],
        );
        let render = |rt: &mut Engine| {
            let mut output = RenderTarget::new([8, 6]).unwrap();
            rt.compose_frame()
                .unwrap()
                .render_into(&mut output, rt.drawing().target.as_ref())
                .unwrap();
            output
        };
        assert_eq!(render(&mut rt).pixel(0, 0), Some(0xff11_2233));
        rt.update(4, 1).unwrap();
        assert_eq!(state(&mut rt, id), 7);
        let output = render(&mut rt);
        assert_eq!(output.pixel(2, 2), Some(0xff55_7733));
        if native == 905 {
            assert_eq!(output.pixel(0, 0), Some(0xffab_cdef));
        } else {
            assert_eq!(output.pixel(0, 0), Some(0xbf5e_7891));
        }
        execute(&mut rt, &[(531, &[id, 0])]);
        rt.update(6, 1).unwrap();
        assert_eq!(render(&mut rt), output);
        execute(&mut rt, &[(531, &[id, 3])]);
        rt.update(14, 1).unwrap();
        render(&mut rt);
        assert_eq!(state(&mut rt, id), 0);
        execute(&mut rt, &[(858, &[1]), (859, &[0, 0, 10f32.to_bits()])]);
        assert_eq!(
            rt.compositor().auxiliary_targets()[0].pixel(0, 0),
            Some(0xffab_cdef)
        );
        rt.destroy(id).unwrap();
        assert!(rt.compositor().auxiliary_targets().is_empty());
        assert_eq!(render(&mut rt).pixel(0, 0), Some(0xffab_cdef));
    }
}
