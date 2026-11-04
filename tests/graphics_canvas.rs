use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{
        CanvasLayout, Drawable, Drawables, DrawingContext, GraphicsFactory, PlaneObject,
        Projection, RenderTarget, Surface, TextureFilter,
    },
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn triangle(tile: u32, points: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> Vec<u8> {
    let mut bytes = Vec::from(0x12345678u32.to_le_bytes());
    bytes.extend(tile.to_le_bytes());
    for i in 0..3 {
        for n in points[i] {
            bytes.extend(n.to_bits().to_le_bytes());
        }
        bytes.extend(0x32123456u32.to_le_bytes());
        for n in uv[i] {
            bytes.extend(n.to_bits().to_le_bytes());
        }
    }
    bytes
}

fn rectangle(tile: u32, size: f32) -> Vec<u8> {
    let mut bytes = triangle(
        tile,
        [[0., 0., 0.], [size, 0., 0.], [size, size, 0.]],
        [[0., 0.], [1., 0.], [1., 1.]],
    );
    bytes.extend(triangle(
        tile,
        [[0., 0., 0.], [size, size, 0.], [0., size, 0.]],
        [[0., 0.], [1., 1.], [0., 1.]],
    ));
    bytes
}

#[test]
fn canvas_records_keep_source_attributes_and_use_the_texture_transform_order() {
    let bytes = triangle(
        3,
        [[2., 4., 6.], [3., 4., 6.], [2., 5., 6.]],
        [[0.125, 0.25]; 3],
    );
    let drawable = Drawable::from_canvas(&bytes, 1).unwrap();
    let original = drawable.canvas_triangles().unwrap()[0];
    assert_eq!((original.tag, original.tile), (0x12345678, 3));
    assert_eq!(original.vertices[0].color, 0x32123456);
    assert_eq!(drawable.bounds(), [[2., 4., 6.], [3., 5., 6.]]);
    assert_eq!(drawable.color(), (u32::MAX, 1));
    let transformed = drawable.transformed_canvas_triangle(0, IDENTITY).unwrap();
    assert_eq!(transformed.vertices[0].position, [1.5, 3.5, 6.]);
    assert_eq!(transformed.vertices[0].color, u32::MAX);
    let mut drawings = Drawables::default();
    drawings.insert(1, drawable).unwrap();
    drawings.set_base_point(1, [1., 2., 5000.]);
    drawings.set_position(1, [3., 4., 5.]);
    drawings.set_scale(1, [2., 3., 4.]);
    drawings.set_color(1, 0, 0);
    let mut parent = IDENTITY;
    parent[12..15].copy_from_slice(&[10., 20., 30.]);
    let transformed = drawings
        .get(1)
        .unwrap()
        .transformed_canvas_triangle(0, parent)
        .unwrap();
    assert_eq!(transformed.vertices[0].position, [14.5, 29.5, 74.]);
    assert_eq!(transformed.vertices[0].color, original.vertices[0].color);
    assert_eq!(transformed.uv, original.uv);
    assert_eq!(
        drawings.get(1).unwrap().canvas_triangles().unwrap()[0],
        original
    );
    assert!(Drawable::from_canvas(&bytes[..79], 1).is_err());
    assert!(Drawable::from_canvas(&[], u32::MAX).is_err());
    let empty = Drawable::from_canvas(&[], 0).unwrap();
    assert_eq!(empty.canvas_triangles(), Some(&[][..]));
    assert_eq!(empty.bounds(), [[100_000_000.; 3], [-100_000_000.; 3]]);
}

#[derive(Default)]
struct DrawingHost(Drawables);
impl Host for DrawingHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        Err(Error::new(0, "未提供合成 native"))
    }
    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.0)
    }
    fn canvas_layout(&self) -> Result<Option<CanvasLayout>> {
        Ok(Some(CanvasLayout::new([4, 4], [2, 2], [4, 4])?))
    }
    fn next_task_child(&self, _: Option<u32>) -> Result<Option<u32>> {
        Ok(None)
    }
    fn dispatch_task_command(
        &mut self,
        command: Command,
        _: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        assert_eq!(
            (command.target, command.code, command.data),
            (0, 34, [0; 3])
        );
        *output = 8;
        Ok(true)
    }
}

fn dispatch(
    plane: &mut PlaneObject,
    host: &mut DrawingHost,
    code: u32,
    data: [u32; 3],
    bytes: Option<&mut [u8]>,
) -> Result<()> {
    let mut output = 0xdeadbeef;
    let outcome = plane.dispatch_command_with_buffers(
        kind::PLANE,
        &Command {
            target: 1,
            sender: 0,
            code,
            data,
        },
        &mut CommandBuffers {
            data0: None,
            data1: bytes,
            external: None,
        },
        &mut output,
        host,
        1,
    )?;
    assert!(outcome.done && !outcome.propagate);
    assert_eq!(output, 0xdeadbeef);
    Ok(())
}

#[test]
fn command_83_replaces_the_drawing_even_when_creation_is_disabled_and_keeps_plane_geometry() {
    let mut host = DrawingHost::default();
    let mut plane = PlaneObject::new(1);
    let mut rect: Vec<_> = [0u32, 0, 0, 4f32.to_bits(), 6f32.to_bits(), 0, 0, 0xff123456]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    dispatch(&mut plane, &mut host, 42, [0; 3], Some(&mut rect)).unwrap();
    dispatch(&mut plane, &mut host, 84, [0, 0, 0], None).unwrap();
    host.0.set_display(1, 1);
    let mut bytes = rectangle(0, 2.);
    dispatch(&mut plane, &mut host, 83, [2, 0, 0], Some(&mut bytes)).unwrap();
    assert_eq!(
        (plane.offset(), plane.size(), plane.shape_kind()),
        ([0.; 2], [4., 6.], 2)
    );
    assert_eq!(host.0.get(1).unwrap().display(), 0);
    assert_eq!(host.0.get(1).unwrap().color(), (u32::MAX, 1));
    assert_eq!(host.0.get(1).unwrap().canvas_triangles().unwrap().len(), 2);
    assert!(
        dispatch(
            &mut plane,
            &mut host,
            83,
            [2, 0, 0],
            Some(&mut bytes[..159])
        )
        .is_err()
    );
    assert!(host.0.get(1).is_none());
    dispatch(&mut plane, &mut host, 83, [0; 3], None).unwrap();
    assert!(
        host.0
            .get(1)
            .unwrap()
            .canvas_triangles()
            .unwrap()
            .is_empty()
    );
    dispatch(&mut plane, &mut host, 3, [0; 3], None).unwrap();
    assert!(host.0.get(1).is_none());
    dispatch(&mut plane, &mut host, 83, [u32::MAX; 3], None).unwrap();
    assert!(host.0.get(1).is_none());
}

#[test]
fn frames_read_canvas_tiles_keep_missing_texture_behavior_and_submit_past_32() {
    let mut red = Surface::new([4, 4]).unwrap();
    red.clear(0xffff0000);
    let canvas = RenderTarget::from_tiles([4, 4], [2, 1], vec![Some(red), None]).unwrap();
    let before = canvas.clone();
    for (tile, expected) in [(0, 0xffff0000), (1, u32::MAX), (u32::MAX, u32::MAX)] {
        let mut data = triangle(tile, [[0.; 3]; 3], [[0.; 2]; 3]).repeat(32);
        data.extend(rectangle(tile, 4.));
        let mut drawings = Drawables::default();
        drawings
            .insert(1, Drawable::from_canvas(&data, 34).unwrap())
            .unwrap();
        drawings.set_display(1, 1);
        let mut output = RenderTarget::new([4, 4]).unwrap();
        assert!(
            drawings
                .render_triangles(&mut output, &Projection::default(), IDENTITY)
                .is_err()
        );
        drawings
            .insert(2, Drawable::rectangle([0.; 3], [1.; 2], 0xff0000ff))
            .unwrap();
        drawings.set_display(2, 1);
        drawings.set_base_point(2, [0., 0., 6000.]);
        assert!(
            drawings
                .render_frame(
                    &mut output,
                    &canvas,
                    &Projection::default(),
                    IDENTITY,
                    TextureFilter::Point
                )
                .unwrap()
        );
        assert_eq!(output.pixel(0, 0), Some(0xff0000ff));
        assert_eq!(output.pixel(3, 3), Some(expected));
    }
    assert_eq!(canvas, before);
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for n in args {
        code.push(3);
        code.extend(n.to_le_bytes());
    }
}

struct Programs {
    canvas_rectangle: bool,
}
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        let mut code = vec![];
        if self.canvas_rectangle {
            for (arguments, count) in [
                (vec![0], 4),
                (vec![999, 2f32.to_bits(), 2f32.to_bits()], 12),
                (vec![1, 2f32.to_bits(), 2f32.to_bits()], 12),
            ] {
                for n in arguments {
                    ins(&mut code, 0x25, &[n]);
                }
                ins(&mut code, 0x2b, &[78, count]);
            }
        }
        for n in [2, 2] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[112, 8]);
        ins(&mut code, 0x26, &[0]);
        for (y, color) in [(0, 0xffff0000), (1, 0xff0000ff)] {
            code.extend([0x25, 0x13, 0, 0, 0, 0]);
            for n in [0, y, 2, 1, color] {
                ins(&mut code, 0x25, &[n]);
            }
            ins(&mut code, 0x2b, &[117, 24]);
        }
        for n in [0, 0, 0, 2, 2] {
            ins(&mut code, 0x25, &[n]);
        }
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        for n in [0, 0, 2, 2] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[115, 40]);
        code.extend([0x25, 0x13, 0, 0, 0, 0]);
        ins(&mut code, 0x2b, &[114, 4]);
        for n in [1, 0x80ffffff, 1] {
            ins(&mut code, 0x25, &[n]);
        }
        ins(&mut code, 0x2b, &[100, 12]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![0; 16], 256)?))
    }
}

#[test]
fn ordinary_image_copy_and_color_commands_reach_an_independent_frame() {
    for (budget, canvas_rectangle) in [(1, false), (4096, false), (1, true), (4096, true)] {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs { canvas_rectangle }),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        let plane = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
        if !canvas_rectangle {
            let mut data = rectangle(0, 2.);
            runtime
                .dispatch_command_with_buffers(
                    0,
                    Command {
                        target: plane,
                        sender: 0,
                        code: 83,
                        data: [2, 0, 0],
                    },
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut data),
                        external: None,
                    },
                )
                .unwrap();
        }
        *runtime.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(RenderTarget::new([2, 2]).unwrap()),
        };
        let script = runtime.create(0, kind::ORDINARY, b"paint", b"").unwrap();
        runtime.update(1, 1).unwrap();
        let vm = runtime.object(script).unwrap().interpreter().unwrap();
        let handle = u32::from_le_bytes(vm.read(0, 4).unwrap().try_into().unwrap());
        assert!(runtime.images().get(handle).is_err());
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(
            runtime
                .drawables()
                .get(plane)
                .unwrap()
                .geometry_transform(true)
                .is_some(),
            canvas_rectangle
        );
        runtime.drawables_mut().set_display(plane, 1);
        let canvas = runtime.drawing().target.clone();
        let mut output = RenderTarget::new([2, 2]).unwrap();
        assert!(
            runtime
                .render_frame(
                    &mut output,
                    &Projection::default(),
                    IDENTITY,
                    TextureFilter::Point
                )
                .unwrap()
        );
        assert_eq!(output.pixel(0, 0), Some(0x40800000));
        assert_eq!(output.pixel(1, 1), Some(0x40000080));
        assert_eq!(runtime.drawing().target, canvas);
        runtime.destroy(plane).unwrap();
        assert!(runtime.drawables().get(plane).is_none());
    }
}

#[test]
fn rectangle_source_updates_recut_uvs_and_leave_wrapper_transforms_independent() {
    let mut host = DrawingHost::default();
    let mut plane = PlaneObject::new(1);
    let mut rect: Vec<_> = [1f32, 1., 3., 4., 4., 0., 0., 0.]
        .into_iter()
        .flat_map(|n| n.to_bits().to_le_bytes())
        .collect();
    dispatch(&mut plane, &mut host, 41, [0; 3], Some(&mut rect)).unwrap();
    assert_eq!(
        (plane.offset(), plane.size(), plane.shape_kind()),
        ([0.; 2], [4.; 2], 1)
    );
    let initial = host.0.get(1).unwrap().canvas_triangles().unwrap().to_vec();
    assert_eq!(initial.len(), 8);
    assert_eq!(host.0.get(1).unwrap().color(), (u32::MAX, 0));

    let mut outer: Vec<_> = [2f32, 1., 0.]
        .into_iter()
        .flat_map(|n| n.to_bits().to_le_bytes())
        .collect();
    dispatch(&mut plane, &mut host, 51, [0; 3], Some(&mut outer)).unwrap();
    let mut source: Vec<_> = [1f32, 0., 0.]
        .into_iter()
        .flat_map(|n| n.to_bits().to_le_bytes())
        .collect();
    dispatch(&mut plane, &mut host, 51, [1, 0, 0], Some(&mut source)).unwrap();
    assert_eq!(host.0.get(1).unwrap().canvas_triangles().unwrap(), initial);
    let mut tile = Surface::new([4, 4]).unwrap();
    tile.clear(u32::MAX);
    let canvas = RenderTarget::from_tiles([4, 4], [2, 2], vec![Some(tile); 4]).unwrap();
    let mut output = RenderTarget::new([8, 8]).unwrap();
    assert!(
        !host
            .0
            .render_frame(
                &mut output,
                &canvas,
                &Projection::default(),
                IDENTITY,
                TextureFilter::Point
            )
            .unwrap()
    );
    assert_eq!(host.0.get(1).unwrap().canvas_triangles().unwrap(), initial);
    host.0.set_display(1, 1);
    assert!(
        host.0
            .render_frame(
                &mut output,
                &canvas,
                &Projection::default(),
                IDENTITY,
                TextureFilter::Point
            )
            .unwrap()
    );
    assert_eq!(output.pixel(3, 2), Some(u32::MAX));
    let drawable = host.0.get(1).unwrap();
    let first = drawable.canvas_triangles().unwrap()[0];
    assert_eq!(first.vertices[0].position, [1., 1., 0.]);
    assert_eq!(first.uv[0], [0.5, 0.25]);
    assert_eq!(
        drawable
            .transformed_canvas_triangle(0, IDENTITY)
            .unwrap()
            .vertices[0]
            .position,
        [2.5, 1.5, 0.]
    );

    // 缩放始终选择包装，首数据项不改变目标。
    let mut scale: Vec<_> = [2f32, 3., 1.]
        .into_iter()
        .flat_map(|n| n.to_bits().to_le_bytes())
        .collect();
    dispatch(&mut plane, &mut host, 66, [1, 0, 0], Some(&mut scale)).unwrap();
    let drawable = host.0.get(1).unwrap();
    assert_eq!(drawable.transform().scale(), [2., 3., 1.]);
    assert_eq!(drawable.geometry_transform(true).unwrap().scale(), [1.; 3]);
    let mut query = vec![0xcc; 12];
    dispatch(&mut plane, &mut host, 62, [1, 0, 0], Some(&mut query)).unwrap();
    assert_eq!(query, source);

    let mut matrix = IDENTITY;
    matrix[12] = 1.;
    let mut bytes: Vec<_> = matrix
        .into_iter()
        .flat_map(|n| n.to_bits().to_le_bytes())
        .collect();
    dispatch(&mut plane, &mut host, 57, [1, 0, 0], Some(&mut bytes)).unwrap();
    assert_eq!(
        host.0
            .get(1)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .matrix_override(),
        Some(&matrix)
    );
    assert!(
        host.0
            .get(1)
            .unwrap()
            .transform()
            .matrix_override()
            .is_none()
    );
    let mut zero = [0; 12];
    dispatch(&mut plane, &mut host, 55, [1, 0, 0], Some(&mut zero)).unwrap();
    host.0.prepare_canvas(1).unwrap();
    assert!(
        host.0
            .get(1)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .matrix_override()
            .is_none()
    );

    dispatch(&mut plane, &mut host, 84, [0; 3], None).unwrap();
    dispatch(&mut plane, &mut host, 41, [0; 3], Some(&mut rect)).unwrap();
    assert!(host.0.get(1).is_none());
    assert_eq!((plane.size(), plane.shape_kind()), ([4.; 2], 1));
    dispatch(&mut plane, &mut host, 83, [0; 3], None).unwrap();
    dispatch(&mut plane, &mut host, 51, [1, 0, 0], Some(&mut source)).unwrap();
    let mut query = vec![0xcc; 12];
    dispatch(&mut plane, &mut host, 62, [1, 0, 0], Some(&mut query)).unwrap();
    assert_eq!(query, [0xcc; 12]);
    assert_eq!(host.0.get(1).unwrap().transform().position(), [0.; 3]);
    assert!(
        Drawable::from_canvas_source(
            Drawable::from_canvas(&[], 0).unwrap(),
            CanvasLayout::new([4, 4], [1, 1], [4, 4]).unwrap()
        )
        .is_err()
    );
}
