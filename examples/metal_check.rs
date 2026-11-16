//! 显式硬件核对：cargo run --release --example metal_check。
//! 需要 macOS Metal 设备；合成纹理与软件帧对照，不纳入默认测试发现。

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    metal::objc::rc::autoreleasepool(check)
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("此检查需要 macOS Metal 设备");
}

#[cfg(target_os = "macos")]
fn check() -> Result<(), Box<dyn std::error::Error>> {
    use katze_system2::graphics::{
        DrawList, FrameTexture, FrameTriangle, GraphicsBackend, ProjectedVertex, Rect,
        RenderTarget, SoftwareBackend, Surface, TextureFilter, TextureSource, metal::Renderer,
    };
    let mut renderer = Renderer::new()?;
    println!("Metal 设备：{}", renderer.device_name());
    let texture = Surface::from_bgra(
        [4, 4],
        (0u8..16)
            .flat_map(|v| [v * 13, 255 - v * 11, v * 7, 31 + v * 13])
            .collect(),
    )?;
    let mut canvas = RenderTarget::from_tiles([4, 4], [2, 1], vec![Some(texture), None])?;
    let triangle = |positions: [[f32; 2]; 3], colors: [u32; 3], texture| FrameTriangle {
        vertices: std::array::from_fn(|i| ProjectedVertex {
            position: [positions[i][0], positions[i][1], 0.5],
            rhw: 1.,
            color: colors[i],
        }),
        texture,
        blend_mode: 0,
        clip: None,
    };
    let background = [
        triangle([[0., 0.], [32., 0.], [32., 24.]], [0xff203040; 3], None),
        triangle([[0., 0.], [0., 24.], [32., 24.]], [0xff203040; 3], None),
    ];
    let mut cases = vec![];
    for offset in [0., 1. / 1024., 1. / 512., 0.25, 0.5, -0.5] {
        let mut triangles = background.to_vec();
        triangles.push(triangle(
            [[1. + offset, 1.], [25. + offset, 1.], [25. + offset, 20.]],
            [0x801234e0; 3],
            None,
        ));
        triangles.push(triangle(
            [[1. + offset, 1.], [1. + offset, 20.], [25. + offset, 20.]],
            [0x801234e0; 3],
            None,
        ));
        cases.push(triangles);
    }
    for uv in [[-0.125, 0.125], [0., 0.5], [1.125, 0.125], [2.125, 0.5]] {
        let mut triangles = background.to_vec();
        let mut textured = triangle(
            [[-4., -4.], [40., -4.], [-4., 40.]],
            [0x8080c040; 3],
            Some(FrameTexture {
                filter: None,
                address: Default::default(),
                source: TextureSource::Canvas(0),
                uv: [uv; 3],
            }),
        );
        textured.clip = Some(Rect::from_xywh(2, 3, 20, 16));
        triangles.push(textured);
        cases.push(triangles);
    }
    let mut gradient = triangle(
        [[1., 1.], [30., 2.], [3., 22.]],
        [0x408020ff, 0xc020ff40, 0xffef8040],
        None,
    );
    gradient.vertices[1].rhw = 0.5;
    gradient.vertices[2].rhw = 2.;
    let mut triangles = background.to_vec();
    triangles.push(gradient);
    cases.push(triangles.clone());
    triangles.last_mut().unwrap().texture = Some(FrameTexture {
        filter: None,
        address: Default::default(),
        source: TextureSource::Canvas(0),
        uv: [[0.17, 0.19], [1.13, 0.87], [-0.21, 1.31]],
    });
    cases.push(triangles);
    cases.push(vec![triangle(
        [[0., 0.], [32., 0.], [0., 24.]],
        [0xff123456; 3],
        Some(FrameTexture {
            filter: None,
            address: Default::default(),
            source: TextureSource::Canvas(1),
            uv: [[0.; 2]; 3],
        }),
    )]);
    cases.push(vec![]);
    let image = std::sync::Arc::new(canvas.tiles()[0].as_ref().unwrap().clone());
    let mut independent = cases[11].clone();
    independent
        .last_mut()
        .unwrap()
        .texture
        .as_mut()
        .unwrap()
        .source = TextureSource::Image(image.clone());
    cases.push(independent.clone());
    for depth in [
        [-0.25; 3],
        [1.25; 3],
        [-0.625, 1.375, -0.625],
        [0.375, 0.375, 2.375],
        [0., 1., 0.5],
    ] {
        for mode in 0..3 {
            let mut clipped = triangle(
                [[-4., -4.], [20., -4.], [-4., 20.]],
                if mode == 0 {
                    [0x8080c040; 3]
                } else {
                    [0x408020ff, 0xc020ff40, 0xffef8040]
                },
                (mode == 2).then(|| FrameTexture {
                    filter: None,
                    address: Default::default(),
                    source: TextureSource::Image(image.clone()),
                    // 避开裁剪插值后恰落 texel 边界的点取样，隔离设备插值精度差异。
                    uv: [[0.173, 0.193], [1.137, 0.877], [-0.217, 1.317]],
                }),
            );
            for (index, vertex) in clipped.vertices.iter_mut().enumerate() {
                vertex.rhw = if depth.iter().all(|z| *z < 0.) || depth.iter().all(|z| *z > 1.) {
                    [-1., 0., f32::NAN][index]
                } else {
                    [1., 0.5, 2.][index]
                };
                vertex.position[2] = depth[index];
            }
            cases.push(vec![clipped]);
        }
    }
    for mode in 0..3 {
        let mut empty = triangle(
            [[4.; 2]; 3],
            if mode == 0 {
                [u32::MAX; 3]
            } else {
                [0x408020ff, 0xc020ff40, 0xffef8040]
            },
            (mode == 2).then(|| FrameTexture {
                filter: None,
                address: Default::default(),
                source: TextureSource::Image(image.clone()),
                uv: [[0.5; 2]; 3],
            }),
        );
        for (index, vertex) in empty.vertices.iter_mut().enumerate() {
            vertex.rhw = [-1., 0., f32::NAN][index];
        }
        cases.push(vec![empty]);
    }
    for blend_mode in 0..6 {
        for textured in [false, true] {
            for alpha in [0, 1, 128, 255] {
                let mut commands = background.to_vec();
                let mut source = triangle(
                    [[2., 2.], [28., 2.], [2., 22.]],
                    [(alpha << 24) | 0x90a0b0; 3],
                    textured.then(|| FrameTexture {
                        filter: None,
                        address: Default::default(),
                        source: TextureSource::Image(image.clone()),
                        uv: [[0.5; 2]; 3],
                    }),
                );
                source.blend_mode = blend_mode;
                commands.push(source);
                cases.push(commands);
            }
        }
    }
    let mut mixed = background.to_vec();
    for filter in [
        TextureFilter::Point,
        TextureFilter::Linear,
        TextureFilter::Point,
    ] {
        let mut command = cases[11].last().unwrap().clone();
        command.texture.as_mut().unwrap().filter = Some(filter);
        mixed.push(command);
    }
    cases.push(mixed);
    let mut comparisons = 0;
    let mut largest_error = 0;
    for filter in [TextureFilter::Point, TextureFilter::Linear] {
        for (case, triangles) in cases.iter().enumerate() {
            let frame = DrawList {
                commands: triangles.clone(),
                filter,
            };
            let reference = SoftwareBackend.render(&frame, &mut canvas, [32, 24])?;
            let expected = reference.tiles()[0].as_ref().unwrap();
            let output = renderer.render(&frame, &mut canvas, [32, 24])?;
            let actual = output.tiles()[0].as_ref().unwrap();
            for (index, (&actual, &expected)) in
                actual.pixels().iter().zip(expected.pixels()).enumerate()
            {
                let error = actual.abs_diff(expected);
                largest_error = largest_error.max(error);
                if error > 1 {
                    return Err(format!("case {case}, {filter:?}, pixel ({}, {}), channel {}: GPU {actual}, CPU {expected}", index / 4 % 32, index / 4 / 32, index % 4).into());
                }
            }
            comparisons += 1;
        }
    }
    let cells = std::sync::Arc::new(Surface::from_bgra(
        [8, 8],
        (0..64u32)
            .flat_map(|i| (0xff000000 | ((i % 8) * 31) | (((i / 8) * 31) << 8)).to_le_bytes())
            .collect(),
    )?);
    let positions = [[-0.5, -0.5], [7.5, -0.5], [-0.5, 5.5], [7.5, 5.5]];
    let uv = [
        [-0.03125, -0.03125],
        [0.46875, -0.03125],
        [-0.03125, 0.34375],
        [0.46875, 0.34375],
    ];
    for filter in [TextureFilter::Point, TextureFilter::Linear] {
        let commands = [[0, 1, 2], [2, 1, 3]].map(|indices| {
            triangle(
                indices.map(|i| positions[i]),
                [u32::MAX; 3],
                Some(FrameTexture {
                    source: TextureSource::Image(cells.clone()),
                    uv: indices.map(|i| uv[i]),
                    filter: Some(TextureFilter::Point),
                    address: Default::default(),
                }),
            )
        });
        let frame = DrawList {
            commands: commands.to_vec(),
            filter,
        };
        let reference = SoftwareBackend.render(&frame, &mut canvas, [8, 6])?;
        let output = renderer.render(&frame, &mut canvas, [8, 6])?;
        let expected = reference.tiles()[0].as_ref().unwrap();
        let actual = output.tiles()[0].as_ref().unwrap();
        assert_eq!(
            actual.pixels(),
            expected.pixels(),
            "affine point sampling, {filter:?}"
        );
        comparisons += 1;
    }
    let triangles = &cases[6];
    let before =
        renderer.render_offscreen(triangles, &mut canvas, [32, 24], TextureFilter::Point)?;
    assert_eq!(
        before,
        renderer.render_offscreen(triangles, &mut canvas, [32, 24], TextureFilter::Point)?
    );
    let mut replacement = Surface::new([4, 4])?;
    replacement.clear(0xffff00ff);
    canvas.copy_surface(&replacement, replacement.bounds(), [0, 0]);
    let after =
        renderer.render_offscreen(triangles, &mut canvas, [32, 24], TextureFilter::Point)?;
    assert_ne!(before, after);
    assert!(canvas.dirty_tiles().iter().all(|dirty| !dirty));
    renderer.invalidate_canvas();
    assert_eq!(
        after,
        renderer.render_offscreen(triangles, &mut canvas, [32, 24], TextureFilter::Point)?
    );
    let before =
        renderer.render_offscreen(&independent, &mut canvas, [32, 24], TextureFilter::Point)?;
    let mut changed = Surface::new([4, 4])?;
    changed.clear(0xff00ff00);
    let changed = std::sync::Arc::new(changed);
    let released = std::sync::Arc::downgrade(&changed);
    independent
        .last_mut()
        .unwrap()
        .texture
        .as_mut()
        .unwrap()
        .source = TextureSource::Image(changed);
    let after =
        renderer.render_offscreen(&independent, &mut canvas, [32, 24], TextureFilter::Point)?;
    assert_ne!(before, after);
    assert_eq!(
        after,
        renderer.render_offscreen(&independent, &mut canvas, [32, 24], TextureFilter::Point)?
    );
    drop(independent);
    renderer.render_offscreen(&[], &mut canvas, [32, 24], TextureFilter::Point)?;
    assert!(released.upgrade().is_none());
    println!("通过 {comparisons} 组合成帧、缓存复用与变更上传；最大通道误差 {largest_error}");
    Ok(())
}
