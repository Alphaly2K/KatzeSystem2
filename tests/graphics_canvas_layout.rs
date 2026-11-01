use katze_system2::graphics::{CanvasLayout, Drawable};

#[test]
fn clipping_keeps_source_order_shared_edges_and_local_uvs() {
    let layout = CanvasLayout::new([4, 4], [2, 2], [4, 4]).unwrap();
    let mut source = Drawable::rectangle([1., 1., 7.], [4., 4.], 0x81234567)
        .triangles()
        .to_vec();
    source[0][1].color = 0;
    let output = layout.triangulate(&source).unwrap();
    assert_eq!(
        output.iter().map(|t| (t.tag, t.tile)).collect::<Vec<_>>(),
        [
            (0, 0),
            (0, 1),
            (0, 1),
            (0, 3),
            (1, 0),
            (1, 2),
            (1, 2),
            (1, 3)
        ]
    );
    assert_eq!(
        output[0].vertices.map(|v| v.position),
        [[1., 1., 0.], [4., 1., 0.], [4., 4., 0.]]
    );
    assert_eq!(output[0].uv, [[0.25, 0.25], [1., 0.25], [1., 1.]]);
    assert_eq!(output[1].uv[0], [0., 0.25]);
    assert_eq!(output[7].uv, [[0., 0.], [0.25, 0.25], [0., 0.25]]);
    assert!(
        output
            .iter()
            .flat_map(|t| t.vertices)
            .all(|v| v.color == 0x81234567)
    );

    let reversed = Drawable::rectangle([5., 5., 0.], [-4., -4.], u32::MAX);
    assert_eq!(layout.triangulate(reversed.triangles()).unwrap().len(), 8);
    for (at, size) in [([0.125, 0.125, 0.], [0.5, 0.5]), ([1., 1., 0.], [0., 4.])] {
        let source = Drawable::rectangle(at, size, 0);
        assert!(layout.triangulate(source.triangles()).unwrap().is_empty());
    }
}

#[test]
fn subdivision_uses_the_baseline_row_divisor_and_rejects_unbounded_work() {
    let layout = CanvasLayout::new([6, 6], [2, 1], [3, 2]).unwrap();
    let source = Drawable::rectangle([0., 0., 0.], [12., 6.], u32::MAX);
    let output = layout.triangulate(source.triangles()).unwrap();
    let last_row = output.iter().find(|t| t.tag == 1 && t.tile == 2).unwrap();
    assert_eq!(last_row.vertices[0].position, [0., 4., 0.]);
    assert_eq!(last_row.uv[0], [0., -1. / 3.]);
    assert_eq!(layout.tile_size(), [6, 6]);
    assert_eq!(layout.grid(), [2, 1]);
    assert_eq!(layout.step(), [3, 2]);

    assert!(CanvasLayout::new([4, 4], [1, 1], [0, 4]).is_err());
    assert!(CanvasLayout::new([4, 4], [1, 1], [5, 4]).is_err());
    assert!(CanvasLayout::new([u32::MAX, 4], [1, 1], [1, 4]).is_err());
    assert!(CanvasLayout::new([1, 1], [1_000_001, 1], [1, 1]).is_err());
    let large = CanvasLayout::new([1, 1], [1000, 1000], [1, 1]).unwrap();
    assert!(large.triangulate(source.triangles()).is_err());
    let mut invalid = source.triangles().to_vec();
    invalid[0][0].position[0] = f32::NAN;
    assert!(layout.triangulate(&invalid).is_err());
    let empty = CanvasLayout::new([4, 4], [0, 1], [4, 4]).unwrap();
    assert!(empty.triangulate(source.triangles()).unwrap().is_empty());
}
