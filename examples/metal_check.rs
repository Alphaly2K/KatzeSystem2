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
        FrameTexture, FrameTriangle, ProjectedVertex, Rect, RenderTarget, Surface, TextureFilter,
        metal::Renderer,
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
                tile: 0,
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
        tile: 0,
        uv: [[0.17, 0.19], [1.13, 0.87], [-0.21, 1.31]],
    });
    cases.push(triangles);
    cases.push(vec![triangle(
        [[0., 0.], [32., 0.], [0., 24.]],
        [0xff123456; 3],
        Some(FrameTexture {
            tile: 1,
            uv: [[0.; 2]; 3],
        }),
    )]);
    cases.push(vec![]);
    let mut comparisons = 0;
    let mut largest_error = 0;
    for filter in [TextureFilter::Point, TextureFilter::Linear] {
        for (case, triangles) in cases.iter().enumerate() {
            let mut expected = Surface::new([32, 24])?;
            for triangle in triangles {
                let before = expected.clone();
                if let Some((source, texture)) = triangle.texture.and_then(|t| {
                    canvas
                        .tiles()
                        .get(t.tile as usize)
                        .and_then(Option::as_ref)
                        .map(|s| (s, t))
                }) {
                    expected.blend_textured_triangle(triangle.vertices, texture.uv, source, filter);
                } else {
                    expected.blend_projected_triangle(triangle.vertices);
                }
                if let Some(clip) = triangle.clip {
                    for y in 0..24 {
                        for x in 0..32 {
                            if x < clip.left || x >= clip.right || y < clip.top || y >= clip.bottom
                            {
                                let at = (y * 32 + x) as usize * 4;
                                expected.pixels_mut()[at..at + 4]
                                    .copy_from_slice(&before.pixels()[at..at + 4]);
                            }
                        }
                    }
                }
            }
            let actual = renderer.render_offscreen(triangles, &mut canvas, [32, 24], filter)?;
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
    println!("通过 {comparisons} 组合成帧、缓存复用与变更上传；最大通道误差 {largest_error}");
    Ok(())
}
