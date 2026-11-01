use katze_system2::input::CoordinateMap;

#[test]
fn coordinate_mapping_truncates_each_stage_and_preserves_outside_points() {
    let map = CoordinateMap {
        device_offset: [0.5, -1.5],
        device_scale: 2.,
        configuration_offset: [1, -2],
        logical_scale: [3., 0.5],
        device_units_per_logical: [1.; 2],
        logical_size: [100; 2],
    };
    assert_eq!(map.to_logical([10; 2]), ([9, 3], true));
    assert_eq!(map.to_logical([-10; 2]), ([-18, -1], false));
    let map = CoordinateMap {
        logical_size: [100; 2],
        ..CoordinateMap::default()
    };
    for (point, inside) in [
        ([0, 0], true),
        ([99, 99], true),
        ([100, 99], false),
        ([0, 100], false),
        ([-1, 0], false),
    ] {
        assert_eq!(map.to_logical(point), (point, inside));
    }
    assert_eq!(CoordinateMap::default().to_logical([0; 2]), ([0; 2], false));
    assert_eq!(
        CoordinateMap {
            logical_size: [-1, 100],
            ..map
        }
        .to_logical([0; 2]),
        ([0; 2], false)
    );
}

#[test]
fn coordinate_mapping_keeps_f32_rounding_wrapping_and_invalid_integer_results() {
    let mut map = CoordinateMap {
        logical_size: [i32::MAX; 2],
        ..CoordinateMap::default()
    };
    assert_eq!(
        map.to_logical([16_777_217, 16_777_219]),
        ([16_777_216, 16_777_220], true)
    );
    map.configuration_offset = [1, -1];
    map.logical_scale = [0.5; 2];
    assert_eq!(
        map.to_logical([i32::MIN; 2]),
        ([1_073_741_824, -1_073_741_824], false)
    );
    map = CoordinateMap {
        device_scale: 0.,
        logical_scale: [0.5, 0.],
        ..CoordinateMap::default()
    };
    assert_eq!(map.to_logical([1, 0]), ([-1_073_741_824, 0], false));
    map = CoordinateMap {
        device_scale: f32::INFINITY,
        logical_size: [1; 2],
        ..CoordinateMap::default()
    };
    assert_eq!(map.to_logical([123, -123]), ([0; 2], true));
    map.device_offset = [f32::NAN, f32::NEG_INFINITY];
    assert_eq!(map.to_logical([123, -123]), ([i32::MIN; 2], false));
    map = CoordinateMap {
        logical_scale: [f32::INFINITY, f32::NAN],
        ..CoordinateMap::default()
    };
    assert_eq!(map.to_logical([0, 1]), ([i32::MIN; 2], false));
}

#[test]
fn reverse_mapping_uses_independent_scales_and_checks_original_logical_bounds() {
    let map = CoordinateMap {
        device_offset: [0.9, -0.9],
        device_scale: 1.5,
        configuration_offset: [1, -2],
        logical_scale: [99.; 2],
        device_units_per_logical: [0.5, 1.5],
        logical_size: [8, 6],
    };
    assert_eq!(map.to_device([7, 5]), ([6, 6], true));
    assert_eq!(map.to_device([8, 6]), ([8, 9], false));
    assert_eq!(map.to_device([-1, 0]), ([2, -3], false));
    let map = CoordinateMap {
        device_offset: [-100.; 2],
        ..map
    };
    assert_eq!(map.to_device([0; 2]), ([-98, -103], true));
}

#[test]
fn reverse_mapping_preserves_reciprocal_rounding_and_integer_wraps() {
    let mut map = CoordinateMap {
        device_scale: 0.5,
        configuration_offset: [1, -1],
        device_units_per_logical: [f32::INFINITY, f32::NAN],
        ..CoordinateMap::default()
    };
    assert_eq!(
        map.to_device([1; 2]),
        ([-1_073_741_824, 1_073_741_824], false)
    );
    map.device_units_per_logical = [1.; 2];
    assert_eq!(
        map.to_device([i32::MAX, i32::MIN]),
        ([-1_073_741_824, 1_073_741_824], false)
    );
    map = CoordinateMap {
        device_scale: 3.1,
        ..CoordinateMap::default()
    };
    assert_eq!(map.to_device([10, -10]), ([30, -30], false));
    map = CoordinateMap {
        device_scale: 0.,
        ..CoordinateMap::default()
    };
    assert_eq!(map.to_device([1, -1]), ([0; 2], false));
    map.device_scale = f32::INFINITY;
    assert_eq!(map.to_device([1, -1]), ([i32::MIN; 2], false));
}
