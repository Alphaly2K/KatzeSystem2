use katze_system2::graphics::{Drawable, Drawables, Vertex};

fn identity() -> [f32; 16] {
    std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
}

#[test]
fn drawing_order_keeps_registration_ties_and_the_original_nan_merge_sequence() {
    let ids = [3, u32::MAX, 0, 77, 11, 0x8000_0000, 9, 5];
    let z = [
        0x3f80_0000,
        0xff80_0000,
        0xffc5_4321,
        0xbf80_0000,
        0x3f80_0000,
        0xffc5_4321,
        0x7f80_0000,
        0x3f80_0000,
    ];
    let mut drawings = Drawables::default();
    for (id, bits) in ids.into_iter().zip(z) {
        assert!(drawings.insert(id, Drawable::default()).unwrap());
        drawings.set_base_point(id, [0.0, 0.0, f32::from_bits(bits)]);
    }
    assert!(!drawings.insert(3, Drawable::default()).unwrap());
    assert_eq!(drawings.get(3).unwrap().transform().base_point()[2], 1.0);
    assert_eq!(
        drawings.order(),
        [u32::MAX, 3, 0, 77, 11, 0x8000_0000, 5, 9]
    );
    for (id, bits) in ids.into_iter().zip(z.into_iter().rev()) {
        drawings.set_base_point(id, [0.0, 0.0, f32::from_bits(bits)]);
    }
    let expected = [9, 11, 3, u32::MAX, 0, 77, 0x8000_0000, 5];
    assert_eq!(drawings.order(), expected);
    drawings.set_color(3, 0x1234_5678, 0);
    assert_eq!(drawings.order(), expected);
    assert!(!drawings.remove(123));

    let mut drawings = Drawables::default();
    for id in [9, 3, 7] {
        drawings.insert(id, Drawable::default()).unwrap();
    }
    assert_eq!(drawings.order(), [9, 3, 7]);
    assert!(drawings.remove(3));
    drawings.insert(3, Drawable::default()).unwrap();
    assert_eq!(drawings.order(), [9, 7, 3]);
    drawings.set_position(9, [0.0, 0.0, -10000.0]);
    assert_eq!(drawings.order(), [9, 7, 3]);
    drawings.set_base_point(3, [0.0, 0.0, -1.0]);
    assert_eq!(drawings.order(), [3, 9, 7]);
    assert!(!drawings.set_base_point(123, [0.0; 3]));
}

#[test]
fn triangle_queries_apply_parent_w_and_raw_color_overrides_without_modifying_source_vertices() {
    let vertices = [
        Vertex {
            position: [0.0, 0.0, 0.0],
            color: 0x0012_3456,
        },
        Vertex {
            position: [1.0, 0.0, 0.0],
            color: 0x7f65_4321,
        },
        Vertex {
            position: [0.0, 1.0, 0.0],
            color: 0xffab_cdef,
        },
    ];
    let mut drawings = Drawables::default();
    drawings
        .insert(7, Drawable::new(vec![vertices]).unwrap())
        .unwrap();
    drawings.set_scale(7, [2.0, 3.0, 1.0]);
    drawings.set_position(7, [5.0, 7.0, 9.0]);
    let mut parent = identity();
    parent[12..15].copy_from_slice(&[10.0, 20.0, 30.0]);
    let drawing = drawings.get(7).unwrap();
    assert_eq!(drawing.color(), (u32::MAX, 0));
    assert_eq!(drawing.display(), 0);
    let transformed = drawing.transformed_triangle(0, parent).unwrap();
    assert_eq!(
        transformed.map(|v| v.position),
        [[25.0, 47.0, 69.0], [27.0, 47.0, 69.0], [25.0, 50.0, 69.0]]
    );
    assert_eq!(transformed.map(|v| v.color), vertices.map(|v| v.color));
    assert_eq!(drawing.transformed_triangle(1, parent), None);
    drawings.set_color(7, 0x0011_2233, u32::MAX);
    assert_eq!(
        drawings
            .get(7)
            .unwrap()
            .transformed_triangle(0, parent)
            .unwrap()
            .map(|v| v.color),
        [0x0011_2233; 3]
    );
    drawings.set_color(7, 0x0065_4321, 0);
    drawings.set_alpha(7, 0x1234);
    assert_eq!(drawings.get(7).unwrap().color(), (0x3465_4321, 1));
    assert_eq!(drawings.get(7).unwrap().triangles(), [vertices]);
}
