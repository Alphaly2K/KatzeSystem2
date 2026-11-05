use katze_system2::{
    Result,
    expression::State,
    graphics::{
        Drawable, Drawables, DrawingContext, GraphicsFactory, Projection, RenderTarget, Surface,
        Vertex,
    },
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[test]
fn flat_triangles_share_diagonals_once_and_keep_device_alpha_rounding() {
    for (alpha, expected) in [
        (0, 0x40802010),
        (1, 0x40802011),
        (2, 0x407f2012),
        (127, 0x5f492a78),
        (128, 0x60492a78),
        (200, 0xab2a30b3),
        (254, 0xfd1234df),
        (255, 0xff1234e0),
    ] {
        let mut surface = Surface::new([8, 8]).unwrap();
        surface.clear(0x40802010);
        for triangle in [
            [[1., 1.], [5., 1.], [5., 5.]],
            [[1., 1.], [1., 5.], [5., 5.]],
        ] {
            assert_eq!(
                surface.blend_triangle(triangle, (alpha << 24) | 0x1234e0),
                alpha != 0
            );
        }
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    surface.pixel(x, y),
                    Some(if (1..5).contains(&x) && (1..5).contains(&y) {
                        expected
                    } else {
                        0x40802010
                    })
                );
            }
        }
    }
    let mut surface = Surface::new([4, 4]).unwrap();
    assert!(surface.blend_triangle(
        [
            [1. + 1. / 1024., 1.],
            [3. + 1. / 1024., 1.],
            [1. + 1. / 1024., 3.]
        ],
        u32::MAX
    ));
    assert_eq!(surface.pixel(1, 1), Some(u32::MAX));
    surface.clear(0);
    assert!(surface.blend_triangle(
        [
            [1. + 1. / 512., 1.],
            [3. + 1. / 512., 1.],
            [1. + 1. / 512., 3.]
        ],
        u32::MAX
    ));
    assert_eq!(surface.pixel(1, 1), Some(0));
    assert_eq!(surface.pixel(2, 1), Some(u32::MAX));
    for points in [
        [[1., 1.]; 3],
        [[10., 10.], [12., 10.], [10., 12.]],
        [[f32::NAN, 0.], [1., 0.], [1., 1.]],
    ] {
        let before = surface.clone();
        assert!(!surface.blend_triangle(points, u32::MAX));
        assert_eq!(surface, before);
    }
}

#[test]
fn tiles_use_global_edges_and_mark_only_covered_surfaces() {
    let mut tiled =
        RenderTarget::from_tiles([4, 4], [2, 2], vec![Some(Surface::new([4, 4]).unwrap()); 4])
            .unwrap();
    let mut whole = Surface::new([8, 8]).unwrap();
    for points in [
        [[4. + 1. / 1024., -1.], [7., 7.], [0., 7.]],
        [[4. + 1. / 512., 2.], [8., 2.], [4. + 1. / 512., 6.]],
    ] {
        tiled.blend_triangle(points, 0x801234e0);
        whole.blend_triangle(points, 0x801234e0);
    }
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(tiled.pixel(x, y), whole.pixel(x, y));
        }
    }
    tiled.clear_dirty();
    assert!(tiled.blend_triangle([[0., 0.], [3., 0.], [0., 3.]], u32::MAX));
    assert_eq!(tiled.dirty_tiles(), &[true, false, false, false]);
    tiled.clear_dirty();
    assert!(!tiled.blend_triangle([[0., 0.], [8., 0.], [0., 8.]], 0x00123456));
    assert_eq!(tiled.dirty_tiles(), &[false; 4]);
    let mut sparse = RenderTarget::from_tiles(
        [4, 4],
        [2, 1],
        vec![None, Some(Surface::new([4, 4]).unwrap())],
    )
    .unwrap();
    assert!(sparse.blend_triangle([[0., 0.], [8., 0.], [8., 4.]], u32::MAX));
    assert_eq!(sparse.pixel(0, 0), None);
    assert_eq!(sparse.dirty_tiles(), &[false, true]);
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
            code.push(op);
            for arg in args {
                code.push(3);
                code.extend(arg.to_le_bytes());
            }
        }
        let mut code = vec![];
        for n in [1, 4f32.to_bits(), 3f32.to_bits(), 0x801234e0] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[79, 16]);
        for n in [1, 1f32.to_bits(), 2f32.to_bits(), 0, 1] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[84, 20]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![], 128)?))
    }
}

#[test]
fn ordinary_rectangle_reaches_pixels_and_drawing_context_gates_frames() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: KcsFactory::new(Programs),
        },
    );
    let plane = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    runtime.create(0, kind::ORDINARY, b"paint", b"").unwrap();
    runtime.update(1, 1).unwrap();
    let mut target = RenderTarget::new([8, 8]).unwrap();
    target.clear_rect(katze_system2::graphics::Rect::from_xywh(0, 0, 8, 8));
    *runtime.drawing_mut() = DrawingContext {
        mode: 0,
        enabled: true,
        active: true,
        target: Some(target),
    };
    assert!(
        !runtime
            .render_triangles(&Projection::default(), IDENTITY)
            .unwrap()
    );
    runtime.drawables_mut().set_display(plane, 1);
    assert!(
        runtime
            .render_triangles(&Projection::default(), IDENTITY)
            .unwrap()
    );
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                runtime.drawing().target.as_ref().unwrap().pixel(x, y),
                Some(if (1..5).contains(&x) && (2..5).contains(&y) {
                    0x40091a70
                } else {
                    0
                })
            );
        }
    }
    let before = runtime.drawing().target.clone();
    runtime.drawing_mut().active = false;
    assert!(
        !runtime
            .render_triangles(&Projection::default(), IDENTITY)
            .unwrap()
    );
    assert_eq!(runtime.drawing().target, before);
    runtime.drawing_mut().active = true;
    runtime.drawables_mut().set_blend_mode(plane, 1);
    assert!(
        runtime
            .render_triangles(&Projection::default(), IDENTITY)
            .unwrap_err()
            .message
            .contains("特殊混合")
    );
}

#[test]
fn render_order_visibility_and_submission_limit_are_kept() {
    let mut drawables = Drawables::default();
    drawables
        .insert(7, Drawable::rectangle([0.; 3], [4., 4.], 0xffff0000))
        .unwrap();
    drawables
        .insert(3, Drawable::rectangle([0.; 3], [2., 2.], 0xff0000ff))
        .unwrap();
    drawables.set_display(7, 1);
    drawables.set_display(3, 1);
    drawables.set_base_point(7, [0., 0., 4000.]);
    let mut target = RenderTarget::new([4, 4]).unwrap();
    drawables
        .render_triangles(&mut target, &Projection::default(), IDENTITY)
        .unwrap();
    assert_eq!(target.pixel(0, 0), Some(0xff0000ff));
    assert_eq!(target.pixel(3, 3), Some(0xffff0000));
    let vertex = |x, y, color| Vertex {
        position: [x, y, 0.],
        color,
    };
    let mut triangles = vec![[vertex(0., 0., 0), vertex(0., 0., 0), vertex(0., 0., 0)]; 32];
    triangles.push([
        vertex(0., 0., u32::MAX),
        vertex(4., 0., u32::MAX),
        vertex(0., 4., u32::MAX),
    ]);
    drawables
        .insert(8, Drawable::new(triangles).unwrap())
        .unwrap();
    drawables.set_display(8, 1);
    assert!(
        drawables
            .render_triangles(&mut target, &Projection::default(), IDENTITY)
            .unwrap()
    );
    assert_eq!(target.pixel(0, 0), Some(0xff0000ff));
}

#[test]
fn projection_origin_selection_reaches_frame_pixels() {
    let mut drawables = Drawables::default();
    drawables
        .insert(1, Drawable::rectangle([0., 0., 30000.], [6., 6.], u32::MAX))
        .unwrap();
    drawables.set_display(1, 1);
    drawables.set_position(1, [8., 8., 0.]);
    let projection = Projection {
        center: [16., 16.],
        camera_depth: 50000.,
        ..Projection::default()
    };
    let mut frames = vec![];
    for (mode, origin) in [
        (0, [8., 8.]),
        (1, [16., 16.]),
        (2, [0., 0.]),
        (u32::MAX, [0., 0.]),
    ] {
        drawables.set_projection_origin_mode(1, mode);
        let mut target = RenderTarget::new([32, 32]).unwrap();
        drawables
            .render_triangles(&mut target, &projection, IDENTITY)
            .unwrap();
        let mut expected = RenderTarget::new([32, 32]).unwrap();
        let drawable = drawables.get(1).unwrap();
        for index in 0..2 {
            let triangle = drawable
                .projected_triangle(index, IDENTITY, &projection, origin, true)
                .unwrap();
            expected.blend_projected_triangle(triangle);
        }
        assert_eq!(target, expected);
        frames.push(target);
    }
    assert_ne!(frames[0], frames[1]);
    assert_ne!(frames[1], frames[2]);
    assert_eq!(frames[2], frames[3]);
}
