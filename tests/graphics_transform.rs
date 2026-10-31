use katze_system2::graphics::Transform;

fn identity() -> [f32; 16] {
    std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
}

#[test]
fn geometry_subtracts_only_base_xy_then_scales_rotates_translates_and_applies_parent_w() {
    let mut transform = Transform::default();
    assert_eq!(transform.base_point(), [0.0, 0.0, 5000.0]);
    transform.set_base_point([2.0, 3.0, 9000.0]);
    transform.set_scale([2.0, 3.0, 4.0]);
    transform.set_position([10.0, 20.0, 30.0]);
    transform.set_matrix([
        0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0,
    ]);
    assert_eq!(
        transform.transform_point([5.0, 7.0, 11.0]),
        [-1.0, 28.0, 77.0, 2.0]
    );
    let mut parent = identity();
    parent[12..15].copy_from_slice(&[100.0, 200.0, 300.0]);
    assert_eq!(
        transform.transform_point_with_parent([5.0, 7.0, 11.0], parent),
        [199.0, 428.0, 677.0, 2.0]
    );
    transform.set_base_point([2.0, 3.0, -123.0]);
    assert_eq!(
        transform.transform_point([5.0, 7.0, 11.0]),
        [-1.0, 28.0, 77.0, 2.0]
    );
}

#[test]
fn vectors_keep_equal_zero_signs_and_rotation_exits_a_bitwise_compared_matrix_override() {
    let mut transform = Transform::default();
    transform.set_base_point([-0.0, -0.0, 5000.0]);
    transform.set_position([-0.0; 3]);
    transform.set_rotation([-0.0; 3]);
    assert_eq!(transform.base_point()[0].to_bits(), 0);
    assert_eq!(transform.position().map(f32::to_bits), [0; 3]);
    assert_eq!(transform.rotation().map(f32::to_bits), [0; 3]);
    assert!(!transform.is_dirty());
    transform.set_position([-0.0, 1.0, 0.0]);
    assert_eq!(transform.position()[0].to_bits(), 0x8000_0000);
    transform.clear_dirty();
    transform.set_position([0.0, 1.0, 0.0]);
    assert!(!transform.is_dirty());
    assert_eq!(transform.position()[0].to_bits(), 0x8000_0000);

    let mut matrix = identity();
    matrix[1] = -0.0;
    transform.set_matrix(matrix);
    transform.clear_dirty();
    transform.set_matrix(matrix);
    assert!(!transform.is_dirty());
    matrix[1] = 0.0;
    transform.set_matrix(matrix);
    assert!(transform.is_dirty());
    transform.set_scale([2.0, 1.0, 1.0]);
    assert!(transform.matrix_override().is_some());
    transform.clear_dirty();
    transform.set_rotation([0.0; 3]);
    assert!(transform.is_dirty());
    assert!(transform.matrix_override().is_none());
    assert_eq!(transform.matrix()[0], 2.0);

    matrix[1] = f32::from_bits(0xffc5_4321);
    transform.set_matrix(matrix);
    transform.clear_dirty();
    transform.set_matrix(matrix);
    assert!(!transform.is_dirty());
}

#[test]
fn xyz_rotation_keeps_x87_reduction_and_nonfinite_results() {
    let mut transform = Transform::default();
    transform.set_scale([2.0, -3.0, 4.0]);
    transform.set_rotation([0.25, -0.5, 0.75]);
    assert_eq!(
        transform.matrix().map(f32::to_bits),
        [
            0x3fa4_61ce,
            0x3f99_2343,
            0x3f75_7744,
            0,
            0x400f_781b,
            0xbff1_2ff5,
            0xbf26_bf04,
            0,
            0xbf2f_5ad8,
            0xbffe_cd18,
            0x4059_ad4d,
            0,
            0,
            0,
            0,
            0x3f80_0000,
        ]
    );
    let mut transform = Transform::default();
    transform.set_rotation([f32::from_bits(0x5e80_0000), 0.0, 0.0]);
    assert_eq!(transform.matrix()[5].to_bits(), 0xbf35_033d);
    assert_eq!(transform.matrix()[6].to_bits(), 0xbf35_06aa);
    for (rotation, expected) in [
        (f32::INFINITY, 0xffc0_0000),
        (f32::from_bits(0x5f00_0000), 0x7fc0_0000),
        (f32::from_bits(0xffc5_4321), 0xffc5_4321),
    ] {
        let mut transform = Transform::default();
        transform.set_rotation([rotation, 0.0, 0.0]);
        assert_eq!(transform.matrix().map(f32::to_bits), [expected; 16]);
    }
    let mut transform = Transform::default();
    transform.set_scale([f32::INFINITY, 1.0, 1.0]);
    assert_eq!(transform.matrix().map(f32::to_bits), [0xffc0_0000; 16]);
}
