use katze_system2::graphics::{Drawable, Projection, Vertex};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[test]
fn projection_keeps_depth_rhw_and_both_matrix_translations() {
    let mut projection = Projection {
        camera_depth: 1000.,
        ..Projection::default()
    };
    projection.coordinate_matrix[12] = 100.;
    projection.coordinate_matrix[13] = -200.;
    projection.matrix[12] = 1.;
    projection.matrix[13] = 2.;
    let vertices = [0., 1000., -1000.].map(|z| Vertex {
        position: [30., 40., z],
        color: 0x80123456,
    });
    let result = projection.project_triangle(vertices, 64000., [0.; 2], true);
    assert_eq!(
        result[0].position,
        [131., -158., f32::from_bits(0x3c7c0fc1)]
    );
    assert_eq!(result[0].rhw, 1.);
    assert_eq!(
        result[1].position,
        [30.101, 39.802, f32::from_bits(0x3c7c0fc1)]
    );
    assert_eq!(result[1].rhw, 0.001);
    assert_eq!(
        result[2].position,
        [-0.101, 0.198, f32::from_bits(0xbc7c0fc1)]
    );
    assert_eq!(result[2].rhw, -0.001);
    assert!(result.iter().all(|v| v.color == 0x80123456));
}

#[test]
fn projection_handles_equal_depth_and_composes_with_drawable_geometry() {
    let projection = Projection {
        camera_depth: 1000.,
        ..Projection::default()
    };
    let vertices = [1000., -0., f32::NAN].map(|z| Vertex {
        position: [30., 40., z],
        color: 0xffabcdef,
    });
    let result = projection.project_triangle(vertices, 63000., [10., 20.], true);
    assert_eq!(
        result[0].position,
        [20010., 20020., f32::from_bits(0x3c7c0fc1)]
    );
    assert_eq!(result[0].rhw, 1.);
    assert_eq!(result[1].position, [30., 40., f32::from_bits(0x3cfc0fc1)]);
    assert_eq!(result[1].rhw, 1.);
    assert!(result[2].position.iter().all(|v| v.is_nan()));
    assert!(result[2].rhw.is_nan());
    assert!(result.iter().all(|v| v.color == 0xffabcdef));

    let drawable = Drawable::rectangle([2., 3., 0.], [4., 5.], 0x80445566);
    let result = drawable
        .projected_triangle(1, IDENTITY, &projection, [0.; 2], true)
        .unwrap();
    assert_eq!(
        result.map(|v| [v.position[0], v.position[1]]),
        [[2., 3.], [2., 8.], [6., 8.]]
    );
    assert!(result.iter().all(|v| v.rhw == 1. && v.color == 0x80445566));
    assert!(
        drawable
            .projected_triangle(2, IDENTITY, &projection, [0.; 2], true)
            .is_none()
    );
}
