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
    rt.update(0, 0).unwrap();
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
