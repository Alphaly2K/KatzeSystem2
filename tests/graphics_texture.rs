use katze_system2::graphics::{
    Compositor, DrawCommand, DrawList, Drawable, Drawables, FrameTexture, GraphicsBackend,
    ProjectedVertex, Projection, Rect, RenderTarget, SoftwareBackend, Surface, TextureFilter,
    TextureSource, Vertex,
};
use std::sync::Arc;

#[test]
fn draw_commands_apply_all_blend_factors_after_texture_modulation() {
    let expected = [
        [0xbf788898, 0xcf636368],
        [0xffa8c0d8, 0xff7c7f88],
        [0xffc0d8f0, 0xffb49081],
        [0x80364658, 0x41291a11],
        [0x005a5a58, 0x00442210],
        [0xff96b6d8, 0xff898a91],
    ];
    let texture =
        Arc::new(Surface::from_bgra([1, 1], 0x80c06030u32.to_le_bytes().to_vec()).unwrap());
    let mut canvas = RenderTarget::new([1, 1]).unwrap();
    let mut backend = SoftwareBackend;
    for mode in 0..6 {
        for textured in [false, true] {
            let background = DrawCommand {
                vertices: vertices().map(|v| ProjectedVertex {
                    color: 0xff607080,
                    ..v
                }),
                texture: None,
                clip: None,
                blend_mode: 0,
            };
            let foreground = DrawCommand {
                vertices: vertices().map(|v| ProjectedVertex {
                    color: if textured { 0x8190a0b0 } else { 0x8090a0b0 },
                    ..v
                }),
                texture: textured.then(|| FrameTexture {
                    source: TextureSource::Image(texture.clone()),
                    uv: [[0.5; 2]; 3],
                }),
                clip: Some(Rect::from_xywh(2, 1, 3, 4)),
                blend_mode: mode,
            };
            let mut frame = DrawList {
                commands: vec![background, foreground],
                filter: TextureFilter::Point,
            };
            let output = backend.render(&frame, &mut canvas, [8, 8]).unwrap();
            assert_eq!(
                output.pixel(3, 2),
                Some(expected[mode as usize][usize::from(textured)])
            );
            assert_eq!(output.pixel(1, 2), Some(0xff607080));
            for vertex in &mut frame.commands[1].vertices {
                vertex.color &= 0xffffff;
            }
            let output = backend.render(&frame, &mut canvas, [8, 8]).unwrap();
            assert_eq!(output.pixel(3, 2), Some(0xff607080));
        }
    }
}

#[test]
fn image_commands_hold_their_pixels_and_mix_with_canvas_textures() {
    let mut source = Surface::new([1, 1]).unwrap();
    source.clear(0xffff0000);
    let mut image = Arc::new(source);
    let mut canvas_pixel = Surface::new([1, 1]).unwrap();
    canvas_pixel.clear(0xff0000ff);
    let mut canvas = RenderTarget::from_tiles([1, 1], [1, 1], vec![Some(canvas_pixel)]).unwrap();
    let frame = DrawList {
        commands: vec![
            DrawCommand {
                vertices: vertices(),
                texture: Some(FrameTexture {
                    source: TextureSource::Image(image.clone()),
                    uv: [[0.5; 2]; 3],
                }),
                blend_mode: 0,
                clip: Some(Rect::from_xywh(0, 0, 4, 8)),
            },
            DrawCommand {
                vertices: vertices(),
                texture: Some(FrameTexture {
                    source: TextureSource::Canvas(0),
                    uv: [[0.5; 2]; 3],
                }),
                blend_mode: 0,
                clip: Some(Rect::from_xywh(4, 0, 4, 8)),
            },
        ],
        filter: TextureFilter::Point,
    };
    let mut copy = frame.clone();
    drop(frame);
    Arc::make_mut(&mut image).clear(0xff00ff00);
    let mut backend = SoftwareBackend;
    let output = backend.render(&copy, &mut canvas, [8, 8]).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                output.pixel(x, y),
                Some(if x < 4 { 0xffff0000 } else { 0xff0000ff })
            );
        }
    }
    copy.commands[0].texture.as_mut().unwrap().source = TextureSource::Image(image);
    let changed = backend.render(&copy, &mut canvas, [8, 8]).unwrap();
    assert_eq!(changed.pixel(1, 1), Some(0xff00ff00));
    assert_eq!(changed.pixel(6, 1), Some(0xff0000ff));
}

#[test]
fn depth_clipping_keeps_visible_color_and_texture_interpolation() {
    let image = Arc::new(texture());
    let mut canvas = RenderTarget::new([1, 1]).unwrap();
    let mut backend = SoftwareBackend;
    for filter in [TextureFilter::Point, TextureFilter::Linear] {
        for mode in 0..3 {
            let mut triangle = vertices();
            for (index, vertex) in triangle.iter_mut().enumerate() {
                vertex.rhw = [1., 0.5, 2.][index];
                if mode != 0 {
                    vertex.color = [0x408020ff, 0xc020ff40, 0xffef8040][index];
                }
            }
            let mut frame = DrawList {
                commands: vec![DrawCommand {
                    vertices: triangle,
                    texture: (mode == 2).then(|| FrameTexture {
                        source: TextureSource::Image(image.clone()),
                        uv: [[0.17, 0.19], [1.13, 0.87], [-0.21, 1.31]],
                    }),
                    blend_mode: 0,
                    clip: None,
                }],
                filter,
            };
            let reference = backend.render(&frame, &mut canvas, [8, 8]).unwrap();
            for (depth, visible) in [
                ([-0.25; 3], 0),
                ([1.25; 3], 0),
                ([-0.625, 1.375, -0.625], 1),
                ([0.375, 0.375, 2.375], 2),
                ([0.; 3], 3),
                ([1.; 3], 3),
            ] {
                for (index, vertex) in frame.commands[0].vertices.iter_mut().enumerate() {
                    vertex.position[2] = depth[index];
                    vertex.rhw = if visible == 0 {
                        [-1., 0., f32::NAN][index]
                    } else {
                        [1., 0.5, 2.][index]
                    };
                }
                let output = backend.render(&frame, &mut canvas, [8, 8]).unwrap();
                for y in 0..8 {
                    for x in 0..8 {
                        let inside = match visible {
                            0 => false,
                            1 => x >= 4,
                            2 => y < 4,
                            _ => true,
                        };
                        assert_eq!(
                            output.pixel(x, y),
                            if inside {
                                reference.pixel(x, y)
                            } else {
                                Some(0)
                            },
                            "mode={mode} filter={filter:?} depth={depth:?} pixel=({x},{y})"
                        );
                    }
                }
            }
            for (index, vertex) in frame.commands[0].vertices.iter_mut().enumerate() {
                vertex.position = [4., 4., 0.5];
                vertex.rhw = [-1., 0., f32::NAN][index];
            }
            let empty = backend.render(&frame, &mut canvas, [8, 8]).unwrap();
            assert_eq!(empty.pixel(4, 4), Some(0));
        }
    }
}

fn texture() -> Surface {
    let mut pixels = vec![];
    for y in 0..4 {
        for x in 0..4 {
            pixels.extend([
                13 + x * 47 + y * 3,
                21 + x * 7 + y * 53,
                9 + x * 31 + y * 17,
                7 + x * 39 + y * 43,
            ]);
        }
    }
    Surface::from_bgra([4, 4], pixels).unwrap()
}

fn vertices() -> [ProjectedVertex; 3] {
    [[-4., -4.], [20., -4.], [-4., 20.]].map(|p| ProjectedVertex {
        position: [p[0], p[1], 0.5],
        rhw: 1.,
        color: u32::MAX,
    })
}

#[test]
fn mirror_sampling_filtering_and_modulation_keep_fractional_channels() {
    for (filter, uv, color, expected) in [
        (
            TextureFilter::Point,
            [-0.125, 0.125],
            u32::MAX,
            0x3e7d2010u32,
        ),
        (TextureFilter::Point, [0.125, 0.125], u32::MAX, 0x3e7d2010),
        (TextureFilter::Point, [1.125, 0.125], u32::MAX, 0x5d732553),
        (TextureFilter::Point, [2.125, 0.5], 0x8080c040, 0x3d6d2c0e),
        (TextureFilter::Point, [0.375, 0.5], u32::MAX, 0x6364552a),
        (TextureFilter::Linear, [-0.125, 0.125], u32::MAX, 0x3e7d2010),
        (TextureFilter::Linear, [0., 0.5], u32::MAX, 0x42663310),
        (TextureFilter::Linear, [0.125, 0.5], 0x8080c040, 0x3c70260e),
        (TextureFilter::Linear, [0.375, 0.5], u32::MAX, 0x54654125),
        (TextureFilter::Linear, [0.5, 1.], 0x8080c040, 0x4d634b13),
        (TextureFilter::Linear, [0.875, 0.5], u32::MAX, 0x9c806279),
        (TextureFilter::Linear, [2.125, 0.5], 0x8080c040, 0x3c70260e),
    ] {
        let mut target = Surface::new([8, 8]).unwrap();
        target.clear(0x40802010);
        let vertices = vertices().map(|v| ProjectedVertex { color, ..v });
        assert!(target.blend_textured_triangle(vertices, [uv; 3], &texture(), filter));
        assert!(
            target
                .pixels()
                .chunks_exact(4)
                .all(|p| p == expected.to_le_bytes())
        );
    }
}

#[test]
fn rhw_corrects_both_texture_coordinates_and_vertex_colors() {
    let mut vertices = vertices();
    for (vertex, (rhw, color)) in
        vertices
            .iter_mut()
            .zip([(1., 0x100000ff), (0.5, 0x8000ff00), (2., 0xffffff00)])
    {
        vertex.rhw = rhw;
        vertex.color = color;
    }
    let mut target = Surface::new([8, 8]).unwrap();
    target.clear(0x40802010);
    assert!(target.blend_textured_triangle(
        vertices,
        [[0.625, 0.625]; 3],
        &texture(),
        TextureFilter::Point,
    ));
    for (at, expected) in [
        ([0, 0], 0x4067261e),
        ([4, 0], 0x44642c1c),
        ([0, 4], 0x4f63351c),
        ([4, 4], 0x56623e17),
        ([7, 7], 0x6d64580b),
    ] {
        assert_eq!(target.pixel(at[0], at[1]), Some(expected));
    }
    let mut texture = Surface::new([2, 1]).unwrap();
    texture
        .pixels_mut()
        .copy_from_slice(&[0, 0, 255, 255, 255, 0, 0, 255]);
    let triangle = [[0., 0.], [4., 0.], [0., 4.]].map(|p| ProjectedVertex {
        position: [p[0], p[1], 0.5],
        rhw: 1.,
        color: u32::MAX,
    });
    let uv = [[0.25, 0.5], [0.75, 0.5], [0.25, 0.5]];
    let mut affine = Surface::new([4, 4]).unwrap();
    affine.blend_textured_triangle(triangle, uv, &texture, TextureFilter::Linear);
    let mut perspective = triangle;
    perspective[1].rhw = 3.;
    let mut corrected = Surface::new([4, 4]).unwrap();
    corrected.blend_textured_triangle(perspective, uv, &texture, TextureFilter::Linear);
    assert_eq!(affine.pixel(1, 1), Some(0xffbf0040));
    assert_eq!(corrected.pixel(1, 1), Some(0xff800080));
}

#[test]
fn textured_tiles_share_global_interpolation_and_invalid_inputs_preserve_pixels() {
    let mut target =
        RenderTarget::from_tiles([4, 4], [2, 2], vec![Some(Surface::new([4, 4]).unwrap()); 4])
            .unwrap();
    let mut whole = Surface::new([8, 8]).unwrap();
    let triangle = [[1. + 1. / 1024., 1.], [7., 1.], [1., 7.]].map(|p| ProjectedVertex {
        position: [p[0], p[1], 0.5],
        rhw: 1.,
        color: 0x8080c040,
    });
    let uv = [[-0.125, 0.125], [2.125, 0.875], [0.375, 1.125]];
    assert!(target.blend_textured_triangle(triangle, uv, &texture(), TextureFilter::Linear));
    whole.blend_textured_triangle(triangle, uv, &texture(), TextureFilter::Linear);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(target.pixel(x, y), whole.pixel(x, y));
        }
    }
    assert_eq!(target.dirty_tiles(), &[true, true, true, false]);
    let before = whole.clone();
    for rhw in [0., -1., f32::INFINITY, f32::NAN] {
        let mut invalid = triangle;
        invalid[1].rhw = rhw;
        assert!(!whole.blend_textured_triangle(invalid, uv, &texture(), TextureFilter::Point));
    }
    let mut invalid_uv = uv;
    invalid_uv[0][1] = f32::NAN;
    assert!(!whole.blend_textured_triangle(triangle, invalid_uv, &texture(), TextureFilter::Point));
    assert!(!whole.blend_textured_triangle(
        triangle,
        uv,
        &Surface::new([0, 4]).unwrap(),
        TextureFilter::Point
    ));
    let transparent = triangle.map(|v| ProjectedVertex {
        color: 0x00123456,
        ..v
    });
    assert!(!whole.blend_textured_triangle(transparent, uv, &texture(), TextureFilter::Linear));
    assert_eq!(whole, before);
}

#[test]
fn drawable_submission_interpolates_colors_without_a_texture() {
    const IDENTITY: [f32; 16] = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    let triangle = [
        Vertex {
            position: [0., 0., 0.],
            color: 0xffff0000,
        },
        Vertex {
            position: [4., 0., 0.],
            color: 0xff00ff00,
        },
        Vertex {
            position: [0., 4., 0.],
            color: 0xff0000ff,
        },
    ];
    let mut drawables = Drawables::default();
    drawables
        .insert(1, Drawable::new(vec![triangle]).unwrap())
        .unwrap();
    drawables.set_display(1, 1);
    let mut target = RenderTarget::new([4, 4]).unwrap();
    assert!(
        drawables
            .render_triangles(&mut target, &Projection::default(), IDENTITY)
            .unwrap()
    );
    assert_eq!(target.pixel(0, 0), Some(0xffff0000));
    assert_eq!(target.pixel(1, 1), Some(0xff804040));
    assert_eq!(target.pixel(2, 2), Some(0));

    let mut compositor = Compositor::default();
    *compositor.drawables_mut() = drawables;
    compositor.drawing_mut().target = Some(RenderTarget::new([4, 4]).unwrap());
    compositor.set_frame_view(Projection::default(), IDENTITY, TextureFilter::Point);
    let frame = compositor.compose(None).unwrap();
    let composited = SoftwareBackend
        .render(
            &frame,
            compositor.drawing_mut().target.as_mut().unwrap(),
            [4, 4],
        )
        .unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(composited.pixel(x, y), target.pixel(x, y));
        }
    }
}
